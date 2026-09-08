//! Arithmetic flag formulas and condition folding over logical-width values.
//! CPU record layout, pending definitions and publication belong to state.

mod condition;
pub(super) use condition::Condition;

use std::convert::Infallible;
use wasm86_compiler::{MemoryInt, Val, I1, I16, I32, I8};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum StatusFlag {
    CF,
    PF,
    AF,
    ZF,
    SF,
    OF,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ArithmeticKind {
    Add,
    Sub,
}

/// Operands and result retain the instruction's logical width. Building a source
/// does not evaluate its flags; each query authors only the expressions it needs.
#[derive(Clone)]
pub(super) struct ArithmeticSource<T: MemoryInt> {
    pub(super) kind: ArithmeticKind,
    pub(super) left: Val<T>,
    pub(super) right: Val<T>,
    pub(super) result: Val<T>,
}

impl<T: MemoryInt> ArithmeticSource<T> {
    pub(super) fn add(left: Val<T>, right: Val<T>) -> Self {
        Self::new(ArithmeticKind::Add, left, right)
    }

    pub(super) fn subtract(left: Val<T>, right: Val<T>) -> Self {
        Self::new(ArithmeticKind::Sub, left, right)
    }

    pub(super) fn new(kind: ArithmeticKind, left: Val<T>, right: Val<T>) -> Self {
        let result = match kind {
            ArithmeticKind::Add => left.add(&right),
            ArithmeticKind::Sub => left.sub(&right),
        };
        Self {
            kind,
            left,
            right,
            result,
        }
    }

    pub(super) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match flag {
            StatusFlag::CF => match self.kind {
                ArithmeticKind::Add => self.result.unsigned().lt(&self.left),
                ArithmeticKind::Sub => self.left.unsigned().lt(&self.right),
            },
            StatusFlag::AF => bit(&self.left.xor(&self.right).xor(&self.result), 4),
            StatusFlag::OF => {
                let left_xor_result = self.left.xor(&self.result);
                let other = match self.kind {
                    ArithmeticKind::Add => self.right.xor(&self.result),
                    ArithmeticKind::Sub => self.left.xor(&self.right),
                };
                bit(&left_xor_result.and(other), T::BYTES * 8 - 1)
            }
            StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(&self.result, flag),
        }
    }

    pub(super) fn condition(&self, condition: Condition) -> Val<I1> {
        // A CMP's relation follows its original operands, including signed
        // overflow cases. These conditions need no intermediate flag image.
        if self.kind == ArithmeticKind::Sub {
            if let Some(compare) = condition.operand_comparison::<T>() {
                return compare(&self.left, &self.right);
            }
        }
        condition
            .evaluate(|flag| Ok::<_, Infallible>(self.flag(flag)))
            .unwrap_or_else(|never| match never {})
    }
}

/// State retains different instruction widths without erasing compiler value types.
#[derive(Clone)]
pub(super) enum ArithmeticFlagSource {
    Byte(ArithmeticSource<I8>),
    Word(ArithmeticSource<I16>),
    Dword(ArithmeticSource<I32>),
}

impl ArithmeticFlagSource {
    pub(super) fn condition(&self, condition: Condition) -> Val<I1> {
        match self {
            Self::Byte(source) => source.condition(condition),
            Self::Word(source) => source.condition(condition),
            Self::Dword(source) => source.condition(condition),
        }
    }
}

impl From<ArithmeticSource<I8>> for ArithmeticFlagSource {
    fn from(source: ArithmeticSource<I8>) -> Self {
        Self::Byte(source)
    }
}
impl From<ArithmeticSource<I16>> for ArithmeticFlagSource {
    fn from(source: ArithmeticSource<I16>) -> Self {
        Self::Word(source)
    }
}
impl From<ArithmeticSource<I32>> for ArithmeticFlagSource {
    fn from(source: ArithmeticSource<I32>) -> Self {
        Self::Dword(source)
    }
}

/// Existing logic records clear CF/OF and use zero for undefined AF. Their other
/// flags use the same result rules as arithmetic.
pub(super) fn logic_flag<T: MemoryInt>(result: &Val<T>, flag: StatusFlag) -> Val<I1> {
    match flag {
        StatusFlag::CF | StatusFlag::OF | StatusFlag::AF => result.and(0).ne(0),
        StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(result, flag),
    }
}

fn result_flag<T: MemoryInt>(result: &Val<T>, flag: StatusFlag) -> Val<I1> {
    match flag {
        StatusFlag::PF => result.and(0xff).popcnt().and(1).eq(0),
        StatusFlag::ZF => result.eq(0),
        StatusFlag::SF => bit(result, T::BYTES * 8 - 1),
        _ => unreachable!("only parity, zero and sign depend on the result alone"),
    }
}

fn bit<T: MemoryInt>(value: &Val<T>, index: u32) -> Val<I1> {
    value.unsigned().shr(index).and(1).ne(0)
}

#[cfg(test)]
mod tests;
