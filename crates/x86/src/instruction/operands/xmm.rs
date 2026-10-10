//! Typed XMM operands defer state and checked memory accesses to execution.
use crate::{
    address::MemoryAddress,
    execution::{ExecutionBuilder, OperandSpan},
    instruction::{Location, Operand},
    memory::{Intent, TransferType},
    register::RegisterCode,
};
use wasm86_compiler::{BuildError, Val, I32, I64, V128};

#[derive(Clone, Copy)]
pub(crate) enum VectorAlignment {
    Unaligned,
    Aligned,
}
impl VectorAlignment {
    fn span(self, bytes: u32) -> OperandSpan {
        match self {
            Self::Unaligned => bytes.into(),
            Self::Aligned => OperandSpan::aligned(bytes),
        }
    }
}

/// An XMM access width: the low I32/I64 lane or the complete V128 register.
/// Writes replace only that portion of the register.
pub(crate) trait XmmType: TransferType {
    fn read_register(
        execution: &mut ExecutionBuilder<'_, '_>,
        register: RegisterCode,
    ) -> Result<Val<Self>, BuildError>;

    fn write_register(
        execution: &mut ExecutionBuilder<'_, '_>,
        register: RegisterCode,
        value: Val<Self>,
    ) -> Result<(), BuildError>;
}

macro_rules! scalar_xmm_types {
    ($($ty:ty),+) => { $(
        impl XmmType for $ty {
            fn read_register(
                execution: &mut ExecutionBuilder<'_, '_>,
                register: RegisterCode,
            ) -> Result<Val<Self>, BuildError> {
                Ok(execution.read_xmm(register)?.extract_lane(0))
            }

            fn write_register(
                execution: &mut ExecutionBuilder<'_, '_>,
                register: RegisterCode,
                value: Val<Self>,
            ) -> Result<(), BuildError> {
                let vector = execution.read_xmm(register.clone())?;
                execution.write_xmm(register, vector.replace_lane(0, value))
            }
        }
    )+};
}
scalar_xmm_types!(I32, I64);

impl XmmType for V128 {
    fn read_register(
        execution: &mut ExecutionBuilder<'_, '_>,
        register: RegisterCode,
    ) -> Result<Val<Self>, BuildError> {
        execution.read_xmm(register)
    }

    fn write_register(
        execution: &mut ExecutionBuilder<'_, '_>,
        register: RegisterCode,
        value: Val<Self>,
    ) -> Result<(), BuildError> {
        execution.write_xmm(register, value)
    }
}

/// A location in the XMM register class or a memory operand.
pub(crate) enum XmmLocation {
    Register(RegisterCode),
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

    /// Reads the low T-width register portion or exactly T::BYTES from memory.
    pub(crate) fn read<T: XmmType>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        alignment: VectorAlignment,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Register(register) => T::read_register(execution, register),
            Self::Memory(address) => execution
                .memory_operand(*address, alignment.span(T::BYTES), Intent::Read, &[])?
                .read(execution, 0),
        }
    }

    /// Writes the low T-width register portion or exactly T::BYTES to memory.
    /// Other register bits and memory bytes are preserved.
    pub(crate) fn write<T: XmmType>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        alignment: VectorAlignment,
        value: Val<T>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Register(register) => T::write_register(execution, register, value),
            Self::Memory(address) => execution
                .memory_operand(*address, alignment.span(T::BYTES), Intent::Write, &[])?
                .write(execution, 0, value),
        }
    }
}
