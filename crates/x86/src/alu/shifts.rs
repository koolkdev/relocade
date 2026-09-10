//! Logical-width shift results and the flags defined for a nonzero count.

use wasm86_compiler::{MemoryInt, Val, I1, I32};

use super::{
    bit,
    flags::{AnyFlagSource, FlagChange, FlagSource, StatusFlag},
    result_flag, AluResult,
};

#[derive(Clone, Copy)]
pub(crate) enum ShiftOp {
    Left,
    RightLogical,
    RightArithmetic,
}

impl ShiftOp {
    /// The caller masks the x86 count to five bits and installs these flags only
    /// when that count is nonzero. The result also remains valid at count zero.
    pub(crate) fn apply<T: MemoryInt>(self, input: Val<T>, count: Val<I32>) -> AluResult<T>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let width = T::BYTES * 8;
        let result = match self {
            Self::Left => input.shl(&count),
            Self::RightLogical => input.unsigned().shr(&count),
            Self::RightArithmetic => input.signed().shr(&count),
        };
        let carry = match self {
            Self::Left => input
                .unsigned()
                .shr(Val::<I32>::from(width).sub(&count))
                .truncate::<I1>()
                .and(count.unsigned().lt(width)),
            Self::RightLogical => input
                .unsigned()
                .shr(count.sub(1))
                .truncate::<I1>()
                .and(count.unsigned().lt(width)),
            Self::RightArithmetic => input.signed().shr(count.sub(1)).truncate::<I1>(),
        };
        let overflow = match self {
            Self::Left => bit(&result, width - 1).xor(&carry),
            Self::RightLogical => bit(&input, width - 1),
            Self::RightArithmetic => false.into(),
        }
        .and(count.eq(1));
        // For nonzero counts, AF is undefined and OF is undefined except at one.
        // SHL/SHR also leave CF undefined at or above the operand width. Choose
        // zero for each undefined flag; none requires reading the prior source.
        let flags = StatusFlag::ALL.map(|flag| match flag {
            StatusFlag::CF => carry.clone(),
            StatusFlag::OF => overflow.clone(),
            StatusFlag::AF => false.into(),
            StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(&result, flag),
        });
        AluResult {
            result,
            flags: FlagChange::from(FlagSource::<T>::Explicit { flags }),
        }
    }
}
