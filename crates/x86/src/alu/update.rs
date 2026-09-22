//! Pure operand modifications shared by register and memory destinations.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::ArithmeticOp;

/// Captured inputs to an operand modification. Flags and other register outputs
/// are derived separately from the value observed by the modification.
pub(crate) enum OperandUpdate<T: MemoryInt> {
    Add(Val<T>),
    AddWithCarry {
        source: Val<T>,
        carry: Val<I1>,
    },
    Subtract(Val<T>),
    SubtractWithBorrow {
        source: Val<T>,
        borrow: Val<I1>,
    },
    And(Val<T>),
    Or(Val<T>),
    Xor(Val<T>),
    Negate,
    Exchange(Val<T>),
    CompareExchange {
        expected: Val<T>,
        replacement: Val<T>,
    },
}

impl<T: MemoryInt> OperandUpdate<T> {
    pub(crate) fn apply(&self, previous: &Val<T>) -> Val<T> {
        match self {
            Self::Add(value) => ArithmeticOp::Add.result(previous, value),
            Self::AddWithCarry { source, carry } => {
                ArithmeticOp::Add.result_with_carry(previous, source, carry)
            }
            Self::Subtract(value) => ArithmeticOp::Subtract.result(previous, value),
            Self::SubtractWithBorrow { source, borrow } => {
                ArithmeticOp::Subtract.result_with_carry(previous, source, borrow)
            }
            Self::And(value) => previous.and(value),
            Self::Or(value) => previous.or(value),
            Self::Xor(value) => previous.xor(value),
            Self::Negate => Val::<T>::from(0).sub(previous),
            Self::Exchange(value) => value.clone(),
            Self::CompareExchange {
                expected,
                replacement,
            } => expected.eq(previous).select(replacement, previous),
        }
    }
}
