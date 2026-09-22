//! x87 state owns stack positions, status updates and publication at guest exits.

mod registers;
mod status;
mod value;

pub(crate) use value::ExtendedValue;

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16, I32};

use crate::ssa::Environment;

use super::access::cpu_location;

#[derive(Clone)]
pub(crate) struct X87State {
    environment: Environment,
    status: status::Status,
    registers: registers::Registers,
}

pub(crate) struct StackValue {
    pub(crate) value: ExtendedValue,
    pub(crate) empty: Val<I1>,
}

impl X87State {
    pub(crate) fn new(memory: Mem) -> Self {
        Self {
            environment: Environment::new(memory),
            status: status::Status::new(memory),
            registers: registers::Registers::new(memory),
        }
    }

    pub(crate) fn control_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.environment.read(body, cpu_location!(x87.control_word))
    }

    pub(crate) fn status_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.status.word(body)
    }

    pub(crate) fn pending_exception(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        self.status.pending(body)
    }

    pub(crate) fn initialize(&mut self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        // FNINIT marks the stack empty without changing register payloads.
        self.environment
            .define(body, cpu_location!(x87.control_word), 0x037f)?;
        self.status.initialize(body)?;
        self.registers.initialize(body)?;
        self.environment
            .define(body, cpu_location!(x87.opcode), 0)?;
        self.environment
            .define(body, cpu_location!(x87.instruction_offset), 0)?;
        self.environment
            .define(body, cpu_location!(x87.data_offset), 0)?;
        self.environment
            .define(body, cpu_location!(x87.instruction_selector), 0)?;
        self.environment
            .define(body, cpu_location!(x87.data_selector), 0)
    }

    pub(crate) fn clear_exceptions(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<(), BuildError> {
        // C0/C1/C2/C3 are undefined for FNCLEX; retain them and the unchanged TOP.
        self.status.clear_exceptions(body)
    }

    pub(crate) fn load_control_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        control: Val<I16>,
    ) -> Result<(), BuildError> {
        self.status.update_pending_exception(body, &control)?;
        self.environment
            .define(body, cpu_location!(x87.control_word), control)
    }

    fn slot(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        index: impl Into<Val<I32>>,
    ) -> Result<Val<I32>, BuildError> {
        let top = self.status.top(body)?;
        body.value(top.add(index).and(7))
    }

    pub(crate) fn read_stack(
        &mut self,
        body: &mut FunctionBuilder<'_>,
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
        body: &mut FunctionBuilder<'_>,
        fault: &Val<I1>,
        overflow: Val<I1>,
    ) -> Result<Val<I1>, BuildError> {
        let control = self.control_word(body)?;
        let unmasked = body.value(fault.and(control.and(1).eq(0)))?;
        self.status.stack_fault(body, fault, &unmasked, overflow)?;
        Ok(unmasked.eq(false))
    }

    pub(crate) fn write_stack(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        index: impl Into<Val<I32>>,
        value: &ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(body, index)?;
        self.registers
            .write(body, &slot, value, value.tag(), enabled)
    }

    pub(crate) fn push(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        value: &ExtendedValue,
        source_empty: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        let source_empty = source_empty.into();
        let top = self.status.top(body)?;
        let target = body.value(top.sub(1).and(7))?;
        let full = self.registers.tag(body, &target)?.ne(3);
        let fault = source_empty.or(&full);
        // A missing source takes priority over an occupied push destination.
        let overflow = source_empty.eq(false).and(full);
        let enabled = self.stack_fault(body, &fault, overflow)?;
        let value = value.or_indefinite(&fault);
        self.registers
            .write(body, &target, &value, value.tag(), &enabled)?;
        self.status.set_top(body, target, enabled)
    }

    pub(crate) fn pop(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let top = self.status.top(body)?;
        self.registers.set_tag(body, &top, 3.into(), enabled)?;
        self.status.set_top(body, top.add(1), enabled)
    }

    pub(crate) fn free(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        index: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        let slot = self.slot(body, index)?;
        self.registers.set_tag(body, &slot, 3.into(), &true.into())
    }

    pub(crate) fn rotate(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        increment: bool,
    ) -> Result<(), BuildError> {
        let top = self.status.top(body)?;
        self.status
            .set_top(body, top.add(if increment { 1 } else { u32::MAX }), true)?;
        self.status.set_c1(body, false)
    }

    pub(crate) fn record_instruction(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        offset: &Val<I32>,
        selector: Val<I16>,
        opcode: &Val<I16>,
    ) -> Result<(), BuildError> {
        self.environment
            .define(body, cpu_location!(x87.instruction_offset), offset)?;
        self.environment
            .define(body, cpu_location!(x87.instruction_selector), selector)?;
        self.environment
            .define(body, cpu_location!(x87.opcode), opcode)
    }

    pub(crate) fn record_data(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        offset: &Val<I32>,
        selector: Val<I16>,
    ) -> Result<(), BuildError> {
        self.environment
            .define(body, cpu_location!(x87.data_offset), offset)?;
        self.environment
            .define(body, cpu_location!(x87.data_selector), selector)
    }

    pub(crate) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.status.publish(body)?;
        self.registers.publish(body)?;
        self.environment.publish(body)
    }
}
