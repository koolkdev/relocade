//! Environment transfers check the complete memory image before changing state.

use super::*;
use crate::{
    address::MemoryAddress,
    memory::{Intent, TransferType},
    state::x87::EnvironmentMode,
    ExecutionProfile,
};
use wasm86_compiler::MemoryInt;

instruction_families! {
    FLDENV {
        execute: load::<_>;
        forms { 0xD9 / 4 => word_or_dword(mem); }
    }
    FNSTENV {
        execute: store::<_>;
        forms { 0xD9 / 6 => word_or_dword(mem); }
    }
}

fn mode(execution: &ExecutionBuilder<'_, '_>) -> EnvironmentMode {
    match execution.profile() {
        ExecutionProfile::Protected(_) => EnvironmentMode::Protected,
        ExecutionProfile::Real16 => EnvironmentMode::Real,
    }
}

fn load<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, 7 * T::BYTES, Intent::Read, &[])?;
    let mut words = std::array::from_fn(|_| Val::<T>::from(0));
    for (index, word) in words.iter_mut().enumerate() {
        *word = operand.read::<T>(execution, index as u32 * T::BYTES)?;
    }
    let mode = mode(execution);
    execution.x87().load_environment(mode, &words)
}

fn store<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let operand = execution.memory_operand(address, 7 * T::BYTES, Intent::Write, &[])?;
    let mode = mode(execution);
    let words = execution.x87().environment::<T>(mode)?;
    for (index, word) in words.iter().enumerate() {
        operand.write(execution, index as u32 * T::BYTES, word)?;
    }
    execution.x87().mask_exceptions()
}
