//! x87 controls and stack operations share waiting-exception boundaries.

mod stack;

pub(crate) use stack::{
    exchange_register, free_register, load_extended, load_register, rotate_stack, store_extended,
    store_register,
};

use wasm86_compiler::{BuildError, I16};

use crate::{exception::Exception, instruction::TypedLocation};

use super::ExecutionBuilder;

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
