//! Packed-lane and whole-vector shifts retain their distinct x86 count rules.

use relocade_compiler::{MemoryInt, Val, VectorLane, VectorType, I32, I64, I8, V128};

#[derive(Clone, Copy)]
pub(crate) enum PackedShiftOp {
    Left,
    RightLogical,
    RightArithmetic,
}

#[derive(Clone, Copy)]
pub(crate) enum ByteShiftOp {
    Left,
    Right,
}

impl PackedShiftOp {
    /// Packed shifts use an unsigned 64-bit count without masking it to the lane width.
    pub(crate) fn apply<T: VectorLane + MemoryInt, V: VectorType>(
        self,
        input: Val<V>,
        count: Val<I64>,
    ) -> Val<V> {
        // Test before narrowing: a nonzero high dword must not become a small count.
        let in_range = count.unsigned().lt(u64::from(T::BYTES * 8));
        let count = count.truncate::<I32>();
        match self {
            Self::Left => in_range.select(input.shl_lanes::<T>(count), 0_u64),
            Self::RightLogical => in_range.select(input.unsigned_shr_lanes::<T>(count), 0_u64),
            Self::RightArithmetic => {
                input.signed_shr_lanes::<T>(in_range.select(count, T::BYTES * 8 - 1))
            }
        }
    }
}

impl ByteShiftOp {
    /// Moves bytes across the whole vector; counts of 16 or more produce zero.
    pub(crate) fn apply(self, input: Val<V128>, count: Val<I8>) -> Val<V128> {
        let in_range = count.unsigned().lt(16);
        let counts = Val::<V128>::splat(count);
        let positions = Val::<V128>::from(std::array::from_fn::<_, 16, _>(|byte| byte as u8));
        let indices = match self {
            // Underflow puts vacated bytes outside the swizzle's input range.
            Self::Left => positions.sub_lanes::<I8>(&counts),
            Self::Right => positions.add_lanes::<I8>(&counts),
        };
        in_range.select(input.swizzle_bytes(indices), 0_u128)
    }
}
