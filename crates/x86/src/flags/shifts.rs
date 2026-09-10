//! Logical-width shift results and the flags defined for a nonzero count.

use wasm86_compiler::{MemoryInt, Val, I1, I32};

use super::{bit, result_flag, FlagSource, StatusFlag};

#[derive(Clone, Copy)]
pub(crate) enum ShiftKind {
    Left,
    RightUnsigned,
    RightSigned,
}

impl<T: MemoryInt> FlagSource<T> {
    /// The caller masks the x86 count to five bits and installs these flags only
    /// when that count is nonzero. The result also remains valid at count zero.
    pub(crate) fn shift(kind: ShiftKind, input: Val<T>, count: Val<I32>) -> Self {
        let width = T::BYTES * 8;
        let result = match kind {
            ShiftKind::Left => input.shl(&count),
            ShiftKind::RightUnsigned => input.unsigned().shr(&count),
            ShiftKind::RightSigned => input.signed().shr(&count),
        };
        let carry = match kind {
            ShiftKind::Left => input
                .unsigned()
                .shr(Val::<I32>::from(width).sub(&count))
                .truncate::<I1>()
                .and(count.unsigned().lt(width)),
            ShiftKind::RightUnsigned => input
                .unsigned()
                .shr(count.sub(1))
                .truncate::<I1>()
                .and(count.unsigned().lt(width)),
            ShiftKind::RightSigned => input.signed().shr(count.sub(1)).truncate::<I1>(),
        };
        let overflow = match kind {
            ShiftKind::Left => bit(&result, width - 1).xor(&carry),
            ShiftKind::RightUnsigned => bit(&input, width - 1),
            ShiftKind::RightSigned => false.into(),
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
        Self::Explicit { result, flags }
    }
}
