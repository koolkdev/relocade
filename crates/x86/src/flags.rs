//! Status-flag sources and condition folding over logical-width values.
//! CPU record layout, pending definitions and publication belong to state.

mod changes;
mod condition;
mod rotates;
mod shifts;
pub(super) use changes::{FlagChange, FlagMask};
pub(super) use condition::Condition;
pub(super) use rotates::RotateKind;
pub(super) use shifts::ShiftKind;

use std::convert::Infallible;

use wasm86_compiler::{MemoryInt, Val, I1, I16, I32, I8};

/// Dense indices for local flag values; CPU record offsets belong to state.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum StatusFlag {
    CF,
    PF,
    AF,
    ZF,
    SF,
    OF,
}

impl StatusFlag {
    pub(super) const ALL: [Self; 6] = [Self::CF, Self::PF, Self::AF, Self::ZF, Self::SF, Self::OF];
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ArithmeticKind {
    Add,
    Sub,
}

/// A complete status source keeps the values its flag rules require.
#[derive(Clone)]
pub(super) enum FlagSource<T: MemoryInt> {
    /// Two operands and their sum or difference retain their logical width.
    Arithmetic {
        kind: ArithmeticKind,
        left: Val<T>,
        right: Val<T>,
        result: Val<T>,
    },
    Logic {
        result: Val<T>,
    },
    /// Symbolic flag values in StatusFlag order; the compiler places evaluation.
    Explicit {
        result: Val<T>,
        flags: [Val<I1>; 6],
    },
}

impl<T: MemoryInt> FlagSource<T> {
    pub(super) fn arithmetic(kind: ArithmeticKind, left: Val<T>, right: Val<T>) -> Self {
        let result = arithmetic_result(kind, &left, &right);
        Self::Arithmetic {
            kind,
            left,
            right,
            result,
        }
    }

    pub(super) fn arithmetic_with_carry(
        kind: ArithmeticKind,
        left: Val<T>,
        right: Val<T>,
        carry_in: Val<I1>,
    ) -> Self {
        let result = arithmetic_result(kind, &left, &right);
        let carry = carry_in.unsigned().extend::<T>();
        let result = arithmetic_result(kind, &result, &carry);
        let flags = StatusFlag::ALL
            .map(|flag| arithmetic_flag(kind, &left, &right, &result, &carry_in, flag));
        Self::Explicit { result, flags }
    }

    pub(super) fn result(&self) -> &Val<T> {
        match self {
            Self::Arithmetic { result, .. }
            | Self::Logic { result }
            | Self::Explicit { result, .. } => result,
        }
    }

    pub(super) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match self {
            Self::Arithmetic {
                kind,
                left,
                right,
                result,
            } => arithmetic_flag(*kind, left, right, result, &false.into(), flag),
            Self::Logic { result } => logic_flag(result, flag),
            Self::Explicit { flags, .. } => flags[flag as usize].clone(),
        }
    }

    pub(super) fn condition(&self, condition: Condition) -> Val<I1> {
        match self {
            Self::Arithmetic {
                kind: ArithmeticKind::Sub,
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

fn arithmetic_result<T: MemoryInt>(kind: ArithmeticKind, left: &Val<T>, right: &Val<T>) -> Val<T> {
    match kind {
        ArithmeticKind::Add => left.add(right),
        ArithmeticKind::Sub => left.sub(right),
    }
}

fn arithmetic_flag<T: MemoryInt>(
    kind: ArithmeticKind,
    left: &Val<T>,
    right: &Val<T>,
    result: &Val<T>,
    carry_in: &Val<I1>,
    flag: StatusFlag,
) -> Val<I1> {
    match flag {
        StatusFlag::CF => {
            // Addition compares its wrapped result; subtraction compares its
            // operands. Incoming carry or borrow decides the equality case.
            let (left, right) = match kind {
                ArithmeticKind::Add => (result, left),
                ArithmeticKind::Sub => (left, right),
            };
            carry_in.select(right.unsigned().ge(left), left.unsigned().lt(right))
        }
        StatusFlag::AF => bit(&left.xor(right).xor(result), 4),
        StatusFlag::OF => {
            let left_xor_result = left.xor(result);
            let other = match kind {
                ArithmeticKind::Add => right.xor(result),
                ArithmeticKind::Sub => left.xor(right),
            };
            bit(&left_xor_result.and(other), T::BYTES * 8 - 1)
        }
        StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(result, flag),
    }
}

/// State retains different source widths without erasing compiler value types.
#[derive(Clone)]
pub(super) enum LocalFlagSource {
    Byte(FlagSource<I8>),
    Word(FlagSource<I16>),
    Dword(FlagSource<I32>),
}

impl LocalFlagSource {
    pub(super) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match self {
            Self::Byte(source) => source.flag(flag),
            Self::Word(source) => source.flag(flag),
            Self::Dword(source) => source.flag(flag),
        }
    }

    pub(super) fn condition(&self, condition: Condition) -> Val<I1> {
        match self {
            Self::Byte(source) => source.condition(condition),
            Self::Word(source) => source.condition(condition),
            Self::Dword(source) => source.condition(condition),
        }
    }
}

impl From<FlagSource<I8>> for LocalFlagSource {
    fn from(source: FlagSource<I8>) -> Self {
        Self::Byte(source)
    }
}
impl From<FlagSource<I16>> for LocalFlagSource {
    fn from(source: FlagSource<I16>) -> Self {
        Self::Word(source)
    }
}
impl From<FlagSource<I32>> for LocalFlagSource {
    fn from(source: FlagSource<I32>) -> Self {
        Self::Dword(source)
    }
}

/// Logical results clear CF/OF and share arithmetic's result flag rules.
pub(super) fn logic_flag<T: MemoryInt>(result: &Val<T>, flag: StatusFlag) -> Val<I1> {
    match flag {
        StatusFlag::CF | StatusFlag::OF => false.into(),
        // AF is architecturally undefined. Zero is our deterministic policy;
        // preserving it would require evaluating the previous flag source.
        StatusFlag::AF => false.into(),
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
    value.unsigned().shr(index).truncate::<I1>()
}

#[cfg(test)]
mod tests;
