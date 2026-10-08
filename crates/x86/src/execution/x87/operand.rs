//! Execution operands describe access; state operands retain resolved evidence.

use wasm86_compiler::{BuildError, Val, I32, I64};

use crate::{
    address::MemoryAddress,
    memory::Intent,
    state::ArithmeticSource,
    x87::{BinaryFormat, BinaryOperand},
};

use super::super::{memory::MemoryOperand, ExecutionBuilder};

// Execution consumes this descriptor immediately; boxing would allocate only
// to move the address into memory resolution.
#[allow(clippy::large_enum_variant)]
pub(crate) enum X87Operand {
    Register(Val<I32>),
    BinaryMemory {
        address: MemoryAddress<Val<I32>>,
        format: BinaryFormat,
    },
}

pub(super) struct ResolvedX87Operand<'memory> {
    pub(super) source: ArithmeticSource,
    pub(super) memory: Option<MemoryOperand<'memory>>,
}

impl<'memory> ExecutionBuilder<'_, 'memory> {
    /// The complete operation checks pending exceptions before resolving a source.
    pub(super) fn resolve_x87_operand(
        &mut self,
        operand: X87Operand,
    ) -> Result<ResolvedX87Operand<'memory>, BuildError> {
        match operand {
            X87Operand::Register(index) => Ok(ResolvedX87Operand {
                source: ArithmeticSource::Register(index),
                memory: None,
            }),
            X87Operand::BinaryMemory { address, format } => {
                let memory = self.memory_operand(address, format.bytes(), Intent::Read, &[])?;
                let source = ArithmeticSource::Binary(memory.read_x87_binary(self, format)?);
                Ok(ResolvedX87Operand {
                    source,
                    memory: Some(memory),
                })
            }
        }
    }
}

impl MemoryOperand<'_> {
    /// Reads the opcode-defined real format after the complete span was checked.
    pub(crate) fn read_x87_binary(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        format: BinaryFormat,
    ) -> Result<BinaryOperand, BuildError> {
        let bits = match format {
            BinaryFormat::Binary32 => self.read::<I32>(execution, 0)?.unsigned().extend::<I64>(),
            BinaryFormat::Binary64 => self.read::<I64>(execution, 0)?,
        };
        Ok(format.decode(&bits))
    }
}
