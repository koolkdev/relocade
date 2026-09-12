//! Logical-width shift results and the flags defined for a nonzero count.

use wasm86_compiler::{AtLeast, MemoryInt, Val, I1, I32};

use super::{bit, result_flag, AluResult};
use crate::alu::{AnyStatusSource, StatusSource};
use crate::flags::{FlagChange, StatusFlag};

#[derive(Clone, Copy)]
pub(crate) enum ShiftOp {
    Left,
    RightLogical,
    RightArithmetic,
}

#[derive(Clone, Copy)]
pub(crate) enum DoubleShiftOp {
    Left,
    Right,
}

impl ShiftOp {
    /// The caller masks the x86 count to five bits. Zero preserves value and flags.
    pub(crate) fn apply<T: MemoryInt>(self, input: Val<T>, count: Val<I32>) -> AluResult<T>
    where
        StatusSource<T>: Into<AnyStatusSource>,
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
        };
        // For nonzero counts, AF is undefined and OF is undefined except at one.
        // SHL/SHR also leave CF undefined at or above the operand width. Choose
        // zero for each undefined flag; none requires reading the prior source.
        result_with_flags(result, carry, overflow, &count)
    }
}

impl DoubleShiftOp {
    /// The caller masks the count to five bits. Zero preserves value and flags.
    /// Word counts above sixteen choose a zero result and zero CF/AF/OF, with
    /// PF/ZF/SF describing that result. The architecture leaves these undefined.
    pub(crate) fn apply<T: MemoryInt>(
        self,
        input: Val<T>,
        source: Val<T>,
        count: Val<I32>,
    ) -> AluResult<T>
    where
        I32: AtLeast<T>,
        StatusSource<T>: Into<AnyStatusSource>,
    {
        let width = T::BYTES * 8;
        let wrap_count = Val::<I32>::from(width).sub(&count);
        let input32 = input.unsigned().extend::<I32>();
        let source32 = source.unsigned().extend::<I32>();
        let shifted = match self {
            Self::Left => input32.shl(&count).or(source32.unsigned().shr(&wrap_count)),
            Self::Right => input32.unsigned().shr(&count).or(source32.shl(&wrap_count)),
        };
        // Wasm wraps a dword shift by 32 to zero. Select the original operand
        // when count is zero so the source cannot contribute through that wrap.
        let result = count.ne(0).select(shifted.truncate::<T>(), &input);
        let carry = match self {
            Self::Left => input.unsigned().shr(wrap_count).truncate::<I1>(),
            Self::Right => input.unsigned().shr(count.sub(1)).truncate::<I1>(),
        }
        .and(count.unsigned().lt(width + 1));
        let overflow = bit(&input, width - 1).xor(bit(&result, width - 1));
        result_with_flags(result, carry, overflow, &count)
    }
}

fn result_with_flags<T: MemoryInt>(
    result: Val<T>,
    carry: Val<I1>,
    overflow: Val<I1>,
    count: &Val<I32>,
) -> AluResult<T>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    let overflow = overflow.and(count.eq(1));
    let flags = StatusFlag::ALL.map(|flag| match flag {
        StatusFlag::CF => carry.clone(),
        StatusFlag::OF => overflow.clone(),
        StatusFlag::AF => false.into(),
        StatusFlag::PF | StatusFlag::ZF | StatusFlag::SF => result_flag(&result, flag),
    });
    AluResult {
        result,
        flags: FlagChange::from(StatusSource::<T>::Explicit { flags }).when(count.ne(0)),
    }
}
