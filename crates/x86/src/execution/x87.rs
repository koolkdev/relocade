//! x87 operations share pending-exception checks and instruction/data-pointer tracking.

mod load;
mod stack;

pub(crate) use load::load_binary;
pub(crate) use stack::{
    exchange_register, free_register, load_extended, load_register, rotate_stack, store_extended,
    store_register,
};

use wasm86_compiler::{BuildError, I16};

use crate::{instruction::TypedLocation, Segment};

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

/// Checks for a deferred x87 exception before executing FWAIT or an instruction
/// that checks exceptions on entry (called a "waiting instruction" by Intel).
pub(crate) fn check_pending_exception(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError> {
    execution
        .state
        .check_x87(&mut execution.body, &execution.eip, execution.completed)
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
    check_pending_exception(execution)?;
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
