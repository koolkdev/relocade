//! x87 operations share waiting exceptions and instruction/data-pointer tracking.

mod load;
mod stack;

pub(crate) use load::load_binary;
pub(crate) use stack::{
    exchange_register, free_register, load_extended, load_register, rotate_stack, store_extended,
    store_register,
};

use wasm86_compiler::{BuildError, I16};

use crate::{exception::Exception, instruction::TypedLocation, Segment};

use super::{memory::MemoryOperand, ExecutionBuilder};

fn record_instruction(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let selector = execution
        .state
        .read_segment_selector(&mut execution.body, &Segment::Cs.into())?;
    let opcode = execution
        .x87_opcode
        .as_ref()
        .expect("x87 forms retain their opcode");
    execution
        .state
        .x87
        .record_instruction(&mut execution.body, &execution.eip, selector, opcode)
}

fn record_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    operand: &MemoryOperand<'_>,
) -> Result<(), BuildError> {
    record_instruction(execution)?;
    let selector = execution
        .state
        .read_segment_selector(&mut execution.body, operand.segment())?;
    execution
        .state
        .x87
        .record_data(&mut execution.body, operand.offset(), selector)
}

pub(crate) fn wait(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let pending = execution.state.x87.pending_exception(&mut execution.body)?;
    execution.fault_if(pending, Exception::FloatingPoint)
}

pub(crate) fn initialize(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.state.x87.initialize(&mut execution.body)
}

pub(crate) fn clear_exceptions(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.state.x87.clear_exceptions(&mut execution.body)
}

pub(crate) fn load_control(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: TypedLocation<I16>,
) -> Result<(), BuildError> {
    // The emulator resolves an already pending exception before the operand
    // access. No new control or summary bits commit if that access faults.
    wait(execution)?;
    let control = source.read(execution)?;
    execution
        .state
        .x87
        .load_control_word(&mut execution.body, control)
}

pub(crate) fn store_control(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let control = execution.state.x87.control_word(&mut execution.body)?;
    destination.write(execution, control)
}

pub(crate) fn store_status(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let status = execution.state.x87.status_word(&mut execution.body)?;
    destination.write(execution, status)
}
