//! MMX reads distinguish their physical register view from the memory access width.

use relocade_compiler::{AtLeast, BuildError, MemoryInt, Val, I32, I64};

use crate::{
    address::MemoryAddress,
    execution::ExecutionBuilder,
    instruction::{Location, Operand},
    memory::{Intent, TransferType},
    register::RegisterCode,
};

pub(crate) enum MmxLocation {
    Register(RegisterCode),
    Memory(Box<MemoryAddress<Val<I32>>>),
}

impl From<RegisterCode> for MmxLocation {
    fn from(register: RegisterCode) -> Self {
        Self::Register(register)
    }
}

impl MmxLocation {
    pub(crate) fn from_operand(operand: Operand<Val<I32>>) -> Self {
        match operand {
            Operand::Location(Location::Mmx(register)) => Self::Register(register),
            Operand::Location(Location::Memory(address)) => Self::Memory(address),
            _ => unreachable!("MMX forms bind MMX registers or memory"),
        }
    }

    /// Reads the low T-width register portion or exactly T::BYTES from memory.
    pub(crate) fn read<T: MemoryInt + TransferType>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError>
    where
        I64: AtLeast<T>,
    {
        match self {
            Self::Register(register) => Ok(execution.read_mmx(register)?.truncate()),
            Self::Memory(address) => execution
                .memory_operand(*address, T::BYTES, Intent::Read, &[])?
                .read(execution, 0),
        }
    }

    /// Writes a whole MMX register or eight memory bytes. Narrow loads must
    /// explicitly extend their value before defining an MMX destination.
    pub(crate) fn write(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: Val<I64>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Register(register) => execution.write_mmx(register, value),
            Self::Memory(address) => execution
                .memory_operand(*address, 8, Intent::Write, &[])?
                .write(execution, 0, value),
        }
    }
}
