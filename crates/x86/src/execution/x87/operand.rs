//! Operand reads preserve source exception evidence and checked memory provenance.

use wasm86_compiler::{BuildError, Val, I1, I32, I64};

use crate::{
    address::MemoryAddress,
    memory::Intent,
    x87::{BinaryFormat, BinaryOperand, BinaryOperands, ExtendedValue},
};

use super::super::{memory::MemoryOperand, ExecutionBuilder};

// Execution consumes this descriptor immediately; boxing would allocate only
// to move the address into memory resolution.
#[allow(clippy::large_enum_variant)]
pub(crate) enum X87Operand {
    Register(Val<I32>),
    Value(ExtendedValue),
    BinaryMemory {
        address: MemoryAddress<Val<I32>>,
        format: BinaryFormat,
    },
}

pub(super) struct X87Operands<'memory> {
    pub(super) values: BinaryOperands,
    pub(super) stack_fault: Val<I1>,
    pub(super) memory: Option<MemoryOperand<'memory>>,
}

impl<'memory> ExecutionBuilder<'_, 'memory> {
    /// The complete operation checks pending exceptions before resolving a source.
    pub(super) fn read_x87_operands(
        &mut self,
        destination: Val<I32>,
        operand: X87Operand,
    ) -> Result<X87Operands<'memory>, BuildError> {
        match operand {
            X87Operand::Register(index) => {
                // Both operands use the entry TOP, including when they alias.
                let left = self.x87().read_stack(destination)?;
                let right = self.x87().read_stack(index)?;
                Ok(X87Operands {
                    values: BinaryOperands::new(&left.value, &right.value),
                    stack_fault: left.empty.or(right.empty),
                    memory: None,
                })
            }
            X87Operand::Value(value) => {
                let left = self.x87().read_stack(destination)?;
                Ok(X87Operands {
                    values: BinaryOperands::new(&left.value, &value),
                    stack_fault: left.empty,
                    memory: None,
                })
            }
            X87Operand::BinaryMemory { address, format } => {
                let memory = self.memory_operand(address, format.bytes(), Intent::Read, &[])?;
                let source = memory.read_x87_binary(self, format)?;
                let left = self.x87().read_stack(destination)?;
                Ok(X87Operands {
                    values: BinaryOperands::from_binary(&left.value, &source),
                    stack_fault: left.empty,
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
