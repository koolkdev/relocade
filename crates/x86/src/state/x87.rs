//! x87 state owns stack positions, status updates and publication at guest exits.

mod control;
mod registers;
mod status;
mod transfer;
mod value;

pub(crate) use transfer::LoadSource;
pub(crate) use value::ExtendedValue;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Val, I1, I16, I32};

use crate::ssa::StateFields;

use super::access::cpu_location;
use control::Exception;

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

    pub(crate) fn control_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.control.word(body)
    }

    pub(crate) fn status_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.status.word(body)
    }

    pub(crate) fn initialize(&mut self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        // FNINIT marks the stack empty without changing register payloads.
        self.control.load_word(body, 0x037f.into())?;
        self.status.initialize(body)?;
        self.registers.initialize(body)?;
        self.metadata.define(body, cpu_location!(x87.opcode), 0)?;
        self.metadata
            .define(body, cpu_location!(x87.instruction_offset), 0)?;
        self.metadata
            .define(body, cpu_location!(x87.data_offset), 0)?;
        self.metadata
            .define(body, cpu_location!(x87.instruction_selector), 0)?;
        self.metadata
            .define(body, cpu_location!(x87.data_selector), 0)
    }

    pub(crate) fn clear_exceptions(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        // C0/C1/C2/C3 are undefined for FNCLEX; retain them and the unchanged TOP.
        // Clearing ES does not prove that a suppressed write succeeded. Publish
        // its conditional result and discard the old slot mapping before resuming.
        self.registers.rebase(body)?;
        self.status.clear_exceptions(body)
    }

    pub(crate) fn load_control_word(
        &mut self,
        body: &mut BlockBuilder<'_>,
        control: Val<I16>,
    ) -> Result<(), BuildError> {
        self.control.load_word(body, control)?;
        self.status
            .update_pending_exception(body, &mut self.control)
    }

    fn slot(
        &mut self,
        body: &mut BlockBuilder<'_>,
        index: impl Into<Val<I32>>,
    ) -> Result<registers::Slot, BuildError> {
        let top = self.status.top(body)?;
        self.registers.slot(body, &top, index.into())
    }

    pub(crate) fn read_stack(
        &mut self,
        body: &mut BlockBuilder<'_>,
        index: impl Into<Val<I32>>,
    ) -> Result<StackValue, BuildError> {
        let slot = self.slot(body, index)?;
        Ok(StackValue {
            empty: self.registers.tag(body, &slot)?.eq(3),
            value: self.registers.read(body, &slot)?,
        })
    }

    /// Records a stack fault and returns whether its data and stack effects commit.
    /// An unmasked fault becomes pending; its producer still retires normally.
    pub(crate) fn stack_fault(
        &mut self,
        body: &mut BlockBuilder<'_>,
        fault: &Val<I1>,
        overflow: Val<I1>,
    ) -> Result<Val<I1>, BuildError> {
        let unmasked =
            self.status
                .record_exception(body, Exception::Invalid, fault, &mut self.control)?;
        self.status.record_stack_fault(body, fault)?;
        self.status
            .record_pending_exception(body, unmasked.clone())?;
        self.status.set_c1(body, overflow)?;
        Ok(unmasked.eq(false))
    }

    pub(crate) fn write_stack(
        &mut self,
        body: &mut BlockBuilder<'_>,
        index: impl Into<Val<I32>>,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(body, index)?;
        self.registers
            .write(body, &slot, value, value.tag(), enabled)
    }

    pub(crate) fn pop(
        &mut self,
        body: &mut BlockBuilder<'_>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let top = self.status.top(body)?;
        let slot = self.registers.slot(body, &top, 0.into())?;
        self.registers.set_tag(body, &slot, 3.into(), enabled)?;
        self.registers.advance(1);
        self.status.set_top(body, top.add(1), enabled)
    }

    pub(crate) fn free(
        &mut self,
        body: &mut BlockBuilder<'_>,
        index: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(body, index)?;
        self.registers.set_tag(body, &slot, 3.into(), &true.into())
    }

    pub(crate) fn rotate(
        &mut self,
        body: &mut BlockBuilder<'_>,
        increment: bool,
    ) -> Result<(), BuildError> {
        let top = self.status.top(body)?;
        self.status
            .set_top(body, top.add(if increment { 1 } else { u32::MAX }), true)?;
        self.registers.advance(if increment { 1 } else { -1 });
        self.status.set_c1(body, false)
    }

    pub(crate) fn record_instruction(
        &mut self,
        body: &mut BlockBuilder<'_>,
        offset: &Val<I32>,
        selector: Val<I16>,
        opcode: &Val<I16>,
    ) -> Result<(), BuildError> {
        self.metadata
            .define(body, cpu_location!(x87.instruction_offset), offset)?;
        self.metadata
            .define(body, cpu_location!(x87.instruction_selector), selector)?;
        self.metadata
            .define(body, cpu_location!(x87.opcode), opcode)
    }

    pub(crate) fn record_data(
        &mut self,
        body: &mut BlockBuilder<'_>,
        offset: &Val<I32>,
        selector: Val<I16>,
    ) -> Result<(), BuildError> {
        self.metadata
            .define(body, cpu_location!(x87.data_offset), offset)?;
        self.metadata
            .define(body, cpu_location!(x87.data_selector), selector)
    }

    pub(crate) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        self.control.publish(body)?;
        self.status.publish(body)?;
        self.registers.publish(body)?;
        self.metadata.publish(body)
    }
}
