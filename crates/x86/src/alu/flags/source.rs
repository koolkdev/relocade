//! Complete flag descriptions retain their logical-width dependencies.

use std::convert::Infallible;

use wasm86_compiler::{MemoryInt, Val, I1, I16, I32, I8};

use super::{Condition, StatusFlag};
use crate::alu::{logic, ArithmeticOp};

#[derive(Clone)]
pub(crate) enum FlagSource<T: MemoryInt> {
    Arithmetic {
        operation: ArithmeticOp,
        left: Val<T>,
        right: Val<T>,
        result: Val<T>,
    },
    Logic {
        result: Val<T>,
    },
    /// Symbolic flag values in StatusFlag order; the compiler places evaluation.
    Explicit {
        flags: [Val<I1>; 6],
    },
}

impl<T: MemoryInt> FlagSource<T> {
    pub(crate) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match self {
            Self::Arithmetic {
                operation,
                left,
                right,
                result,
            } => operation.flag(left, right, result, &false.into(), flag),
            Self::Logic { result } => logic::flag(result, flag),
            Self::Explicit { flags } => flags[flag as usize].clone(),
        }
    }

    pub(crate) fn condition(&self, condition: Condition) -> Val<I1> {
        match self {
            Self::Arithmetic {
                operation: ArithmeticOp::Subtract,
                left,
                right,
                ..
            } => {
                // Plain subtraction relations follow the operands, including
                // signed overflow. Other sources use their flag expressions.
                if let Some(compare) = condition.operand_comparison::<T>() {
                    return compare(left, right);
                }
            }
            Self::Logic { result } => {
                if let Some(compare) = condition.logic_result_comparison::<T>() {
                    return compare(result);
                }
            }
            Self::Arithmetic { .. } | Self::Explicit { .. } => {}
        }
        match condition.evaluate(|flag| Ok::<_, Infallible>(self.flag(flag))) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }
}

/// A source of any x86 operand width, retaining each compiler value's type.
#[derive(Clone)]
pub(crate) enum AnyFlagSource {
    Byte(FlagSource<I8>),
    Word(FlagSource<I16>),
    Dword(FlagSource<I32>),
}

impl AnyFlagSource {
    pub(crate) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match self {
            Self::Byte(source) => source.flag(flag),
            Self::Word(source) => source.flag(flag),
            Self::Dword(source) => source.flag(flag),
        }
    }

    pub(crate) fn condition(&self, condition: Condition) -> Val<I1> {
        match self {
            Self::Byte(source) => source.condition(condition),
            Self::Word(source) => source.condition(condition),
            Self::Dword(source) => source.condition(condition),
        }
    }
}

impl From<FlagSource<I8>> for AnyFlagSource {
    fn from(source: FlagSource<I8>) -> Self {
        Self::Byte(source)
    }
}
impl From<FlagSource<I16>> for AnyFlagSource {
    fn from(source: FlagSource<I16>) -> Self {
        Self::Word(source)
    }
}
impl From<FlagSource<I32>> for AnyFlagSource {
    fn from(source: FlagSource<I32>) -> Self {
        Self::Dword(source)
    }
}
