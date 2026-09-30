//! Numeric loads convert their source after the complete memory access guard.

use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I32, I64};

use crate::{
    address::MemoryAddress,
    memory::Intent,
    state::LoadSource,
    x87::{BinaryFormat, ExtendedValue},
};

use super::{check_pending_exception, record_memory, ExecutionBuilder};

pub(crate) fn load_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Read, &[])?;
    let bits = match format {
        BinaryFormat::Binary32 => operand
            .read::<I32>(execution, 0)?
            .unsigned()
            .extend::<I64>(),
        BinaryFormat::Binary64 => operand.read::<I64>(execution, 0)?,
    };
    let source = format.decode(&bits);
    execution.specialize_on(|execution| {
        let available = execution.state.x87.push_available(&mut execution.body)?;
        Ok(source
            .signaling_nan
            .eq(false)
            .and(source.denormal.eq(false))
            .and(available))
    })?;
    record_memory(execution, &operand)?;
    execution
        .state
        .x87
        .push(&mut execution.body, LoadSource::Binary(source))
}

pub(crate) fn load_integer<T: MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, T::BYTES, Intent::Read, &[])?;
    let integer = operand.read::<T>(execution, 0)?.signed().extend::<I64>();
    execution.specialize_on(|execution| execution.state.x87.push_available(&mut execution.body))?;
    let value = ExtendedValue::from_signed_integer(&integer);
    record_memory(execution, &operand)?;
    execution
        .state
        .x87
        .push(&mut execution.body, LoadSource::Value(value))
}
