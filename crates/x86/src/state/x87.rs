//! x87 state owns stack positions, status updates and publication at guest exits.

mod control;
mod registers;
mod status;
mod value;

pub(crate) use value::{BinaryFormat, ExtendedValue};

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16, I32};

use crate::ssa::Environment;

use super::access::cpu_location;
use control::Exception;

#[derive(Clone)]
pub(crate) struct X87State {
    environment: Environment,
    control: control::Control,
    status: status::Status,
    registers: registers::Registers,
}

pub(crate) struct StackValue {
    pub(crate) value: ExtendedValue,
    pub(crate) empty: Val<I1>,
}

/// Source provenance determines which exceptions FLD can raise. Raw extended
/// and register transfers do not classify SNaNs or denormals as operands.
pub(crate) enum LoadSource {
    Extended(ExtendedValue),
    Register(StackValue),
    Binary(value::BinaryOperand),
}

impl X87State {
    pub(crate) fn new(memory: Mem) -> Self {
        Self {
            environment: Environment::new(memory),
            control: control::Control::new(memory),
            status: status::Status::new(memory),
            registers: registers::Registers::new(memory),
        }
    }

    pub(crate) fn control_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.control.word(body)
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
        self.control.load_word(body, 0x037f.into())?;
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
        self.control.load_word(body, control)?;
        self.status
            .update_pending_exception(body, &mut self.control)
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
        source: LoadSource,
    ) -> Result<(), BuildError> {
        let (value, source_empty, signaling_nan, denormal) = match source {
            LoadSource::Extended(value) => (value, false.into(), false.into(), false.into()),
            LoadSource::Register(source) => {
                (source.value, source.empty, false.into(), false.into())
            }
            LoadSource::Binary(source) => (
                source.value,
                false.into(),
                source.signaling_nan,
                source.denormal,
            ),
        };
        let top = self.status.top(body)?;
        let target = body.value(top.sub(1).and(7))?;
        let full = self.registers.tag(body, &target)?.ne(3);
        let fault = source_empty.or(&full);
        // A missing source takes priority over an occupied push destination.
        let overflow = source_empty.eq(false).and(full);
        // A stack fault suppresses source conversion exceptions even when the
        // invalid-operation exception is masked.
        let invalid = fault.or(signaling_nan);
        let denormal = invalid.eq(false).and(denormal);
        let unmasked_invalid =
            self.status
                .record_exception(body, Exception::Invalid, &invalid, &mut self.control)?;
        let unmasked_denormal = self.status.record_exception(
            body,
            Exception::Denormal,
            &denormal,
            &mut self.control,
        )?;
        self.status.record_stack_fault(body, &fault)?;
        self.status
            .record_pending_exception(body, unmasked_invalid.or(unmasked_denormal))?;
        self.status.set_c1(body, overflow)?;
        // FLD completes a denormal load even with DM clear (Intel Vol. 2,
        // FLD description). Only an unmasked invalid exception suppresses it.
        let enabled = unmasked_invalid.eq(false);
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
        self.control.publish(body)?;
        self.status.publish(body)?;
        self.registers.publish(body)?;
        self.environment.publish(body)
    }
}
