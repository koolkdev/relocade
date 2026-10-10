//! Typed XMM operands defer state and checked memory accesses to execution.
use crate::{
    address::MemoryAddress,
    execution::{ExecutionBuilder, OperandSpan},
    instruction::{Location, Operand},
    memory::{Intent, TransferType},
};
use wasm86_compiler::{BuildError, Val, VectorLane, I32, V128};

#[derive(Clone, Copy)]
pub(crate) enum VectorAlignment {
    Unaligned,
    Aligned,
}
impl VectorAlignment {
    fn span(self) -> OperandSpan {
        match self {
            Self::Unaligned => 16.into(),
            Self::Aligned => OperandSpan::aligned(16),
        }
    }
}

/// A location in the XMM register class or a memory operand.
pub(crate) enum XmmLocation {
    Register(crate::register::RegisterCode),
    Memory(Box<MemoryAddress<Val<I32>>>),
}

impl XmmLocation {
    pub(crate) fn from_operand(operand: Operand<Val<I32>>) -> Self {
        match operand {
            Operand::Location(Location::Xmm(register)) => Self::Register(register),
            Operand::Location(Location::Memory(address)) => Self::Memory(address),
            _ => unreachable!("XMM forms bind XMM registers or memory"),
        }
    }

    pub(crate) fn is_memory(&self) -> bool {
        matches!(self, Self::Memory(_))
    }

    /// Reads the low register lane or exactly one scalar from memory.
    pub(crate) fn read_scalar<T: VectorLane + TransferType>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Register(register) => Ok(execution.read_xmm(register)?.extract_lane::<T>(0)),
            Self::Memory(address) => execution
                .memory_operand(*address, T::BYTES, Intent::Read, &[])?
                .read(execution, 0),
        }
    }

    pub(crate) fn read_vector(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        alignment: VectorAlignment,
    ) -> Result<Val<V128>, BuildError> {
        match self {
            Self::Register(register) => execution.read_xmm(register),
            Self::Memory(address) => execution
                .memory_operand(*address, alignment.span(), Intent::Read, &[])?
                .read(execution, 0),
        }
    }

    pub(crate) fn write_vector(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        alignment: VectorAlignment,
        value: Val<V128>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Register(register) => execution.write_xmm(register, value),
            Self::Memory(address) => execution
                .memory_operand(*address, alignment.span(), Intent::Write, &[])?
                .write(execution, 0, value),
        }
    }
}
