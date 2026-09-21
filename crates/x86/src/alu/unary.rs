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
    ByteSwap,
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
            Self::ByteSwap => {
                let result = match T::BYTES {
                    // Intel leaves the word result undefined. Choose low-word
                    // clearing, as observed on Intel and AMD processors:
                    // https://gynvael.coldwind.pl/?id=268
                    2 => 0.into(),
                    4 => input
                        .rotl(8)
                        .and(0x00ff_00ff)
                        .or(input.rotr(8).and(0xff00_ff00u32)),
                    _ => unreachable!("BSWAP has only word and dword forms"),
                };
                return AluResult {
                    result,
                    flags: FlagChange::partial([]),
                };
            }
        };
        let outcome = arithmetic.apply(input, 1);
        AluResult {
            result: outcome.result,
            flags: outcome.flags.preserving(StatusFlag::CF),
        }
    }
}
