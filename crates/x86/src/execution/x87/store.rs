//! Converted stores guard the complete destination before conversion or state effects.

use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I32, I64};

use crate::{
    address::MemoryAddress,
    memory::Intent,
    x87::{BinaryFormat, ConversionResult, ExtendedValue, RoundingMode},
};

use super::{check_pending_exception, record_memory, ExecutionBuilder};

pub(crate) fn store_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
    pop: bool,
) -> Result<(), BuildError> {
    let convert = |value: &ExtendedValue, rounding: &RoundingMode| format.encode(value, rounding);
    match format {
        BinaryFormat::Binary32 => store::<I32>(execution, address, pop, convert),
        BinaryFormat::Binary64 => store::<I64>(execution, address, pop, convert),
    }
}

pub(crate) fn store_integer<T: MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    pop: bool,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    store::<T>(
        execution,
        address,
        pop,
        ExtendedValue::to_signed_integer::<T>,
    )
}

fn store<T: MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    pop: bool,
    convert: impl FnOnce(&ExtendedValue, &RoundingMode) -> ConversionResult,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, T::BYTES, Intent::Write, &[])?;
    record_memory(execution, &operand)?;
    let store = execution
        .state
        .x87
        .prepare_store(&mut execution.body, pop, convert)?;
    execution.if_value::<()>(
        &store.enabled,
        |arm| operand.write(arm, 0, store.bits.truncate::<T>()),
        |_| Ok(()),
    )
}
