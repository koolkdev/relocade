//! Narrow stores guard the complete destination before conversion or state effects.

use wasm86_compiler::{BuildError, Val, I32};

use crate::{address::MemoryAddress, memory::Intent, x87::BinaryFormat};

use super::{check_pending_exception, record_memory, ExecutionBuilder};

pub(crate) fn store_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
    pop: bool,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Write, &[])?;
    record_memory(execution, &operand)?;
    let store = execution
        .state
        .x87
        .store_binary(&mut execution.body, format, pop)?;
    execution.if_value::<()>(
        &store.enabled,
        |arm| match format {
            BinaryFormat::Binary32 => operand.write(arm, 0, store.bits.truncate::<I32>()),
            BinaryFormat::Binary64 => operand.write(arm, 0, &store.bits),
        },
        |_| Ok(()),
    )
}
