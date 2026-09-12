//! Unary operand results and their complete, partial or absent flag changes.

use wasm86_compiler::{MemoryInt, Val};

use super::{AluResult, ArithmeticOp};
use crate::alu::{AnyStatusSource, StatusSource};
use crate::flags::{FlagChange, StatusFlag};

#[derive(Clone, Copy)]
pub(crate) enum UnaryOp {
    Increment,
    Decrement,
    Negate,
    Not,
}

impl UnaryOp {
    pub(crate) fn apply<T: MemoryInt>(self, input: Val<T>) -> AluResult<T>
    where
        StatusSource<T>: Into<AnyStatusSource>,
    {
        let arithmetic = match self {
            Self::Increment => ArithmeticOp::Add,
            Self::Decrement => ArithmeticOp::Subtract,
            Self::Negate => return ArithmeticOp::Subtract.apply(0, input),
            Self::Not => {
                return AluResult {
                    result: input.xor(-1),
                    flags: FlagChange::partial([]),
                }
            }
        };
        let outcome = arithmetic.apply(input, 1);
        AluResult {
            result: outcome.result,
            flags: outcome.flags.preserving(StatusFlag::CF),
        }
    }
}
