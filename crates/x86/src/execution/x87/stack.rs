//! Extended transfers commit raw values only after guest memory guards succeed.

use wasm86_compiler::{BuildError, Val, I16, I32, I64};

use crate::{
    address::MemoryAddress,
    instruction::X87StackIndex,
    memory::Intent,
    state::LoadSource,
    x87::{ExtendedBits, ExtendedValue},
};

use super::{check_pending_exception, record_instruction, record_memory, ExecutionBuilder};

pub(crate) fn load_extended(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, 10, Intent::Read, &[])?;
    let value = ExtendedValue::from_bits(ExtendedBits {
        significand: operand.read::<I64>(execution, 0)?,
        sign_exponent: operand.read::<I16>(execution, 8)?,
    });
    record_memory(execution, &operand)?;
    execution
        .state
        .x87
        .push(&mut execution.body, LoadSource::Extended(value))
}

pub(crate) fn load_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    // The source uses the old TOP, including when the push destination aliases it.
    let source = execution
        .state
        .x87
        .read_stack(&mut execution.body, source.offset())?;
    execution
        .state
        .x87
        .push(&mut execution.body, LoadSource::Register(source))
}

pub(crate) fn store_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: X87StackIndex,
    pop: bool,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    let source = execution.state.x87.read_stack(&mut execution.body, 0)?;
    let enabled =
        execution
            .state
            .x87
            .stack_fault(&mut execution.body, &source.empty, false.into())?;
    execution.state.x87.write_stack(
        &mut execution.body,
        destination.offset(),
        &source.value.or_indefinite(&source.empty),
        &enabled,
    )?;
    if pop {
        execution.state.x87.pop(&mut execution.body, &enabled)?;
    }
    Ok(())
}

pub(crate) fn store_extended(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, 10, Intent::Write, &[])?;
    let source = execution.state.x87.read_stack(&mut execution.body, 0)?;
    record_memory(execution, &operand)?;
    let enabled =
        execution
            .state
            .x87
            .stack_fault(&mut execution.body, &source.empty, false.into())?;
    let value = source.value.or_indefinite(&source.empty).bits();
    execution.if_value::<()>(
        &enabled,
        |arm| {
            operand.write(arm, 0, &value.significand)?;
            operand.write(arm, 8, &value.sign_exponent)
        },
        |_| Ok(()),
    )?;
    execution.state.x87.pop(&mut execution.body, &enabled)
}

pub(crate) fn exchange_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    let index = other.offset();
    let top = execution.state.x87.read_stack(&mut execution.body, 0)?;
    let other = execution
        .state
        .x87
        .read_stack(&mut execution.body, &index)?;
    let fault = top.empty.or(&other.empty);
    let enabled = execution
        .state
        .x87
        .stack_fault(&mut execution.body, &fault, false.into())?;
    // Each empty source is replaced before exchanging; the live source survives.
    execution.state.x87.write_stack(
        &mut execution.body,
        0,
        &other.value.or_indefinite(&other.empty),
        &enabled,
    )?;
    execution.state.x87.write_stack(
        &mut execution.body,
        index,
        &top.value.or_indefinite(&top.empty),
        &enabled,
    )
}

pub(crate) fn free_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    register: X87StackIndex,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    execution
        .state
        .x87
        .free(&mut execution.body, register.offset())
}

pub(crate) fn rotate_stack(
    execution: &mut ExecutionBuilder<'_, '_>,
    increment: bool,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    execution.state.x87.rotate(&mut execution.body, increment)
}
