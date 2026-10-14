//! Environment transfers check the complete memory image before changing state.

use super::*;
use crate::{
    address::MemoryAddress,
    memory::{Intent, TransferType},
    state::x87::EnvironmentMode,
    x87::ExtendedBits,
    ExecutionProfile,
};
use wasm86_compiler::{MemoryInt, I64};

instruction_families! {
    FLDENV {
        execute: load::<_>(ImageKind::Environment);
        forms { 0xD9 / 4 => word_or_dword(mem); }
    }
    FNSTENV {
        execute: store::<_>(ImageKind::Environment);
        forms { 0xD9 / 6 => word_or_dword(mem); }
    }
    FRSTOR {
        execute: load::<_>(ImageKind::FullState);
        forms { 0xDD / 4 => word_or_dword(mem); }
    }
    FNSAVE {
        execute: store::<_>(ImageKind::FullState);
        forms { 0xDD / 6 => word_or_dword(mem); }
    }
}

#[derive(Clone, Copy)]
enum ImageKind {
    Environment,
    FullState,
}

impl ImageKind {
    fn bytes<T: MemoryInt>(self) -> u32 {
        7 * T::BYTES
            + match self {
                Self::Environment => 0,
                Self::FullState => 80,
            }
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
    kind: ImageKind,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, kind.bytes::<T>(), Intent::Read, &[])?;
    let mut words = std::array::from_fn(|_| Val::<T>::from(0));
    for (index, word) in words.iter_mut().enumerate() {
        *word = operand.read::<T>(execution, index as u32 * T::BYTES)?;
    }
    let registers = match kind {
        ImageKind::Environment => None,
        ImageKind::FullState => {
            let mut registers = std::array::from_fn(|_| ExtendedBits {
                significand: 0.into(),
                sign_exponent: 0.into(),
            });
            for (index, register) in registers.iter_mut().enumerate() {
                let offset = 7 * T::BYTES + index as u32 * 10;
                *register = ExtendedBits {
                    significand: operand.read::<I64>(execution, offset)?,
                    sign_exponent: operand.read::<I16>(execution, offset + 8)?,
                };
            }
            Some(registers)
        }
    };
    let mode = mode(execution);
    execution.x87().restore(mode, &words, registers.as_ref())
}

fn store<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    kind: ImageKind,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let operand = execution.memory_operand(address, kind.bytes::<T>(), Intent::Write, &[])?;
    let mode = mode(execution);
    let words = execution.x87().environment::<T>(mode)?;
    let registers = match kind {
        ImageKind::Environment => None,
        ImageKind::FullState => Some(execution.x87().stack_bits()?),
    };
    for (index, word) in words.iter().enumerate() {
        operand.write(execution, index as u32 * T::BYTES, word)?;
    }
    if let Some(registers) = registers {
        for (index, register) in registers.iter().enumerate() {
            let offset = 7 * T::BYTES + index as u32 * 10;
            operand.write(execution, offset, &register.significand)?;
            operand.write(execution, offset + 8, &register.sign_exponent)?;
        }
    }
    match kind {
        ImageKind::Environment => execution.x87().mask_exceptions(),
        ImageKind::FullState => execution.x87().initialize(),
    }
}
