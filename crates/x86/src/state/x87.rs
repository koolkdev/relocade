//! x87 state owns stack positions, status updates and publication at guest exits.

mod control;
mod environment;
mod mmx;
mod registers;
mod status;

pub(crate) use environment::EnvironmentMode;
pub(crate) use status::ConditionCode;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32};

use crate::{ssa::StateFields, x87::ExtendedValue};

use super::{access::cpu_location, StoredX87};
pub(crate) use control::Exception;

#[derive(Clone, Copy)]
pub(crate) enum X87Specialization {
    /// Rounding control alone, for operations independent of PC.
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
    pub(crate) tag: Val<I16>,
}

impl StackValue {
    /// Classifies a computed value; raw register copies may retain a saved tag.
    pub(crate) fn from_value(value: ExtendedValue) -> Self {
        Self {
            tag: value.classification().tag(),
            value,
        }
    }

    pub(crate) fn is_empty(&self) -> Val<I1> {
        self.tag.eq(3)
    }
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
/// status backing and publication remain owned by the underlying state.
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
        let top = self.state.registers.top(self.body)?;
        self.state.status.word(self.body, top)
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
        self.state.registers.slot(self.body, index.into())
    }

    pub(crate) fn read_stack(
        &mut self,
        index: impl Into<Val<I32>>,
    ) -> Result<StackValue, BuildError> {
        let slot = self.slot(index)?;
        Ok(StackValue {
            tag: self.state.registers.tag(self.body, &slot)?,
            value: self.state.registers.read(self.body, &slot)?,
        })
    }

    /// Records underflow and returns whether data and stack effects are enabled.
    /// C1 policy is chosen by the instruction independently of these effects.
    pub(crate) fn stack_underflow(&mut self, fault: &Val<I1>) -> Result<Val<I1>, BuildError> {
        let unmasked = self.record_exception(Exception::Invalid, fault)?;
        self.record_stack_fault(fault)?;
        self.record_pending_exception(unmasked.clone())?;
        Ok(unmasked.eq(false))
    }

    fn push_tag(&mut self) -> Result<Val<I16>, BuildError> {
        let target = self.slot(7)?;
        self.state.registers.tag(self.body, &target)
    }

    pub(crate) fn push_available(&mut self) -> Result<Val<I1>, BuildError> {
        Ok(self.push_tag()?.eq(3))
    }

    /// Whether the push destination is occupied; source underflow can still
    /// take priority over an overflow response.
    pub(crate) fn push_full(&mut self) -> Result<Val<I1>, BuildError> {
        Ok(self.push_tag()?.ne(3))
    }

    /// Writes the entry ST(7) slot, then advances TOP when enabled.
    /// A disabled push must leave a pending exception: register tracking assumes
    /// the advance succeeded until the next x87 exception check.
    pub(crate) fn push(
        &mut self,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        self.push_when(value, &true.into(), enabled)
    }

    /// A conditional push may leave TOP unchanged without a pending exception.
    /// Publish that choice before reusing relative stack slots; a later exception
    /// check may discard suppression guards but cannot assume this push happened.
    pub(crate) fn push_when(
        &mut self,
        value: &ExtendedValue,
        condition: &Val<I1>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let constant = self.body.constant_bits(condition)?;
        if constant == Some(0) {
            return Ok(());
        }
        let enabled = condition.and(enabled);
        let target = self.slot(7)?;
        self.state.registers.write(
            self.body,
            &target,
            value,
            value.classification().tag(),
            &enabled,
        )?;
        self.state.registers.advance(self.body, -1, &enabled)?;
        if constant != Some(1) {
            self.state.registers.rebase(self.body)?;
        }
        Ok(())
    }

    pub(crate) fn write_stack(
        &mut self,
        index: impl Into<Val<I32>>,
        value: &StackValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(index)?;
        self.state
            .registers
            .write(self.body, &slot, &value.value, value.tag.clone(), enabled)
    }

    /// An ordinary condition remains part of the value and tag after later
    /// exception checks discard the separate exception-suppression guard.
    pub(crate) fn write_stack_when(
        &mut self,
        index: impl Into<Val<I32>>,
        value: &StackValue,
        condition: &Val<I1>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let index = index.into();
        match self.body.constant_bits(condition)? {
            Some(0) => Ok(()),
            Some(1) => self.write_stack(index, value, enabled),
            _ => {
                let previous = self.read_stack(index.clone())?;
                let value = StackValue {
                    value: value.value.select(condition, &previous.value),
                    tag: condition.select(&value.tag, previous.tag),
                };
                self.write_stack(index, &value, enabled)
            }
        }
    }

    /// A disabled pop must leave a pending exception, as for `push`.
    pub(crate) fn pop(&mut self, enabled: &Val<I1>) -> Result<(), BuildError> {
        self.pop_n(1, enabled)
    }

    /// Uses the same exception suppression contract as `pop`.
    pub(crate) fn pop_n(&mut self, count: u32, enabled: &Val<I1>) -> Result<(), BuildError> {
        if count == 0 {
            return Ok(());
        }
        // All tags use the entry TOP; the movement commits only after every
        // tag has been captured, including the two-slot comparison pop.
        for index in 0..count {
            let slot = self.slot(index)?;
            self.state
                .registers
                .set_tag(self.body, &slot, 3.into(), enabled)?;
        }
        self.state
            .registers
            .advance(self.body, count as i32, enabled)
    }

    pub(crate) fn free(&mut self, index: impl Into<Val<I32>>) -> Result<(), BuildError> {
        let slot = self.slot(index)?;
        self.state
            .registers
            .set_tag(self.body, &slot, 3.into(), &true.into())
    }

    pub(crate) fn rotate(&mut self, increment: bool) -> Result<(), BuildError> {
        self.state
            .registers
            .advance(self.body, if increment { 1 } else { -1 }, true)
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
