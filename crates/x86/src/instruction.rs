mod forms;
mod lower;

pub(crate) use forms::*;
pub(super) use lower::lower;

use crate::{address::Address32, flags::Condition, register::RegisterCode};

pub(super) const MAX_INSTRUCTION_BYTES: u32 = 15;
pub(super) const OPERAND_SIZE_PREFIX: u8 = 0x66;
pub(super) const EXTENDED_OPCODE_ESCAPE: u8 = 0x0f;

#[derive(Clone, Copy)]
pub(super) enum BinaryOperation {
    Mov,
    Add,
    AddWithCarry,
    Subtract,
    SubtractWithBorrow,
    And,
    Or,
    Xor,
    Compare,
    Test,
}

#[derive(Clone, Copy)]
pub(super) enum UnaryOperation {
    Increment,
    Decrement,
    Negate,
    Not,
}

/// Width of the instruction's data operands; effective addresses remain 32-bit.
#[derive(Clone, Copy)]
pub(super) enum OperandWidth {
    Byte,
    Word,
    Dword,
}

impl OperandWidth {
    pub(super) const fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Dword => 4,
        }
    }
}

/// The effective operand-size attribute in the supported default-32 mode.
#[derive(Clone, Copy)]
pub(super) enum OperandSize {
    Word,
    Dword,
}

/// Immediate payloads contain decoded bits; the instruction width gives them
/// their logical data type. Address components continue to use 32-bit values.
pub(super) enum Operand<V> {
    Immediate(V),
    Location(Location<V>),
}

#[derive(Clone)]
pub(super) enum Location<V> {
    Register(RegisterCode),
    Memory(Address32<V>),
}

impl<V> From<Location<V>> for Operand<V> {
    fn from(location: Location<V>) -> Self {
        Self::Location(location)
    }
}

/// Binary operands in Intel order. The operation determines whether the left
/// location is written; CMP and TEST only read it.
pub(super) struct BinaryInstruction<V> {
    pub(super) operation: BinaryOperation,
    pub(super) width: OperandWidth,
    pub(super) left: Location<V>,
    pub(super) right: Operand<V>,
}

pub(super) struct UnaryInstruction<V> {
    pub(super) operation: UnaryOperation,
    pub(super) width: OperandWidth,
    pub(super) destination: Location<V>,
}

pub(super) enum Instruction<V> {
    Binary(BinaryInstruction<V>),
    Unary(UnaryInstruction<V>),
    SetCondition {
        condition: Condition,
        destination: Location<V>,
    },
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) eip: P,
    pub(super) next_eip: P,
}

impl<V> Instruction<V> {
    pub(super) fn uses_memory(&self) -> bool {
        match self {
            Self::Binary(instruction) => {
                matches!(instruction.left, Location::Memory(_))
                    || matches!(instruction.right, Operand::Location(Location::Memory(_)))
            }
            Self::Unary(instruction) => matches!(instruction.destination, Location::Memory(_)),
            Self::SetCondition { destination, .. } => matches!(destination, Location::Memory(_)),
        }
    }
}
