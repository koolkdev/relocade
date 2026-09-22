//! Binary32 and binary64 loads classify their source after the full memory guard.

use wasm86_compiler::{BuildError, Val, I32, I64};

use crate::{
    address::MemoryAddress,
    memory::Intent,
    state::{BinaryFormat, LoadSource},
};

use super::{record_memory, wait, ExecutionBuilder};

pub(crate) fn load_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    wait(execution)?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Read, &[])?;
    let bits = match format {
        BinaryFormat::Binary32 => operand
            .read::<I32>(execution, 0)?
            .unsigned()
            .extend::<I64>(),
        BinaryFormat::Binary64 => operand.read::<I64>(execution, 0)?,
    };
    let source = format.decode(&bits);
    record_memory(execution, &operand)?;
    execution
        .state
        .x87
        .push(&mut execution.body, LoadSource::Binary(source))
}
