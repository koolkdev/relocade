//! x87 state owns stack positions, status updates and publication at guest exits.

mod arithmetic;
mod control;
mod registers;
mod status;
mod transfer;

pub(crate) use arithmetic::{Arithmetic, ArithmeticSource};
pub(crate) use transfer::LoadSource;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32};

use crate::{ssa::StateFields, x87::ExtendedValue};

use super::{access::cpu_location, StoredX87};
use control::Exception;

#[derive(Clone, Copy)]
pub(crate) enum X87Specialization {
    /// Rounding control alone, for conversion stores.
    Rounding,
    /// Precision and rounding, plus PM and PE when both were observed set.
    Arithmetic,
}

#[derive(Clone)]
pub(crate) struct X87State {
    metadata: StateFields,
    control: control::Control,
    status: status::Status,
    registers: registers::Registers,
}

pub(crate) struct StackValue {
    pub(crate) value: ExtendedValue,
    pub(crate) empty: Val<I1>,
}

impl super::State<'_> {
    /// Delivers a deferred x87 exception at this instruction's restart boundary.
    /// The fault path keeps conditional writes. Reaching the continuation proves
    /// that earlier writes were enabled, so later stack reads can omit their guards.
    pub(crate) fn check_x87(
        &mut self,
        body: &mut BlockBuilder<'_>,
        restart_eip: &Val<I32>,
        completed: u32,
    ) -> Result<(), BuildError> {
        let pending = self.x87.status.pending(body)?;
        body.if_(pending, |fault_body| {
            self.fault(
                fault_body,
                restart_eip,
                completed,
                crate::exception::Exception::FloatingPoint,
            )
        })?;
        self.x87.registers.discard_write_guards(body)
    }
}

impl X87State {
    pub(crate) fn new(memory: Mem) -> Self {
        Self {
            metadata: StateFields::new(memory),
            control: control::Control::new(memory),
            status: status::Status::new(memory),
            registers: registers::Registers::new(memory),
        }
    }

    pub(crate) fn access<'state, 'body>(
        &'state mut self,
        body: &'state mut BlockBuilder<'body>,
    ) -> X87Access<'state, 'body> {
        X87Access { state: self, body }
    }

    pub(crate) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.control.publish(body)?;
        self.status.publish(body)?;
        self.registers.publish(body)?;
        self.metadata.publish(body)
    }
}

/// Builds x87 state operations in the current execution body. Register tracking,
/// exception responses and publication remain owned by the underlying state.
pub(crate) struct X87Access<'state, 'body> {
    state: &'state mut X87State,
    body: &'state mut BlockBuilder<'body>,
}

impl X87Access<'_, '_> {
    pub(crate) fn specialization_condition(
        &mut self,
        observed: &StoredX87,
        specialization: X87Specialization,
    ) -> Result<Val<I1>, BuildError> {
        let mut condition =
            self.state
                .control
                .matches_controls(self.body, &observed.control, specialization)?;
        if matches!(specialization, X87Specialization::Arithmetic)
            && observed.control.precision_mask & observed.status.precision & 1 != 0
        {
            // A masked, already-set PE cannot change on another inexact result.
            // Guard both facts so repeated exception calculations fold away while
            // C1 and the numerical result keep their normal rounding behavior.
            condition = condition
                .and(
                    self.state
                        .control
                        .unmasked(self.body, Exception::Precision)?
                        .eq(false),
                )
                .and(
                    self.state
                        .status
                        .exception_raised(self.body, Exception::Precision)?,
                );
        }
        Ok(condition)
    }

    pub(crate) fn control_word(&mut self) -> Result<Val<I16>, BuildError> {
        self.state.control.word(self.body)
    }

    pub(crate) fn status_word(&mut self) -> Result<Val<I16>, BuildError> {
        self.state.status.word(self.body)
    }

    pub(crate) fn initialize(&mut self) -> Result<(), BuildError> {
        // FNINIT marks the stack empty without changing register payloads.
        self.state.control.load_word(self.body, 0x037f.into())?;
        self.state.status.initialize(self.body)?;
        self.state.registers.initialize(self.body)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.opcode), 0)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.instruction_offset), 0)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.data_offset), 0)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.instruction_selector), 0)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.data_selector), 0)
    }

    pub(crate) fn clear_exceptions(&mut self) -> Result<(), BuildError> {
        // C0/C1/C2/C3 are undefined for FNCLEX; retain them and the unchanged TOP.
        // Clearing ES does not prove that a suppressed write succeeded. Publish
        // its conditional result and discard the old slot mapping before resuming.
        self.state.registers.rebase(self.body)?;
        self.state.status.clear_exceptions(self.body)
    }

    pub(crate) fn load_control_word(&mut self, control: Val<I16>) -> Result<(), BuildError> {
        self.state.control.load_word(self.body, control)?;
        self.state
            .status
            .update_pending_exception(self.body, &mut self.state.control)
    }

    fn slot(&mut self, index: impl Into<Val<I32>>) -> Result<registers::Slot, BuildError> {
        let top = self.state.status.top(self.body)?;
        self.state.registers.slot(self.body, &top, index.into())
    }

    pub(crate) fn read_stack(
        &mut self,
        index: impl Into<Val<I32>>,
    ) -> Result<StackValue, BuildError> {
        let slot = self.slot(index)?;
        Ok(StackValue {
            empty: self.state.registers.tag(self.body, &slot)?.eq(3),
            value: self.state.registers.read(self.body, &slot)?,
        })
    }

    /// Records a stack fault and returns whether its data and stack effects commit.
    /// An unmasked fault becomes pending; its producer still retires normally.
    pub(crate) fn stack_fault(
        &mut self,
        fault: &Val<I1>,
        overflow: Val<I1>,
    ) -> Result<Val<I1>, BuildError> {
        let unmasked = self.state.status.record_exception(
            self.body,
            Exception::Invalid,
            fault,
            &mut self.state.control,
        )?;
        self.state.status.record_stack_fault(self.body, fault)?;
        self.state
            .status
            .record_pending_exception(self.body, unmasked.clone())?;
        self.state.status.set_c1(self.body, overflow)?;
        Ok(unmasked.eq(false))
    }

    pub(crate) fn write_stack(
        &mut self,
        index: impl Into<Val<I32>>,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(index)?;
        self.state
            .registers
            .write(self.body, &slot, value, value.tag(), enabled)
    }

    pub(crate) fn pop(&mut self, enabled: &Val<I1>) -> Result<(), BuildError> {
        let top = self.state.status.top(self.body)?;
        let slot = self.state.registers.slot(self.body, &top, 0.into())?;
        self.state
            .registers
            .set_tag(self.body, &slot, 3.into(), enabled)?;
        self.state.registers.advance(1);
        self.state.status.set_top(self.body, top.add(1), enabled)
    }

    pub(crate) fn free(&mut self, index: impl Into<Val<I32>>) -> Result<(), BuildError> {
        let slot = self.slot(index)?;
        self.state
            .registers
            .set_tag(self.body, &slot, 3.into(), &true.into())
    }

    pub(crate) fn rotate(&mut self, increment: bool) -> Result<(), BuildError> {
        let top = self.state.status.top(self.body)?;
        self.state.status.set_top(
            self.body,
            top.add(if increment { 1 } else { u32::MAX }),
            true,
        )?;
        self.state.registers.advance(if increment { 1 } else { -1 });
        self.state.status.set_c1(self.body, false)
    }

    pub(crate) fn record_instruction(
        &mut self,
        offset: &Val<I32>,
        selector: Val<I16>,
        opcode: &Val<I16>,
    ) -> Result<(), BuildError> {
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.instruction_offset), offset)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.instruction_selector), selector)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.opcode), opcode)
    }

    pub(crate) fn record_data(
        &mut self,
        offset: &Val<I32>,
        selector: Val<I16>,
    ) -> Result<(), BuildError> {
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.data_offset), offset)?;
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.data_selector), selector)
    }
}
