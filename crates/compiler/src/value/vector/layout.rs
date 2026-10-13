//! Byte selection and lane access composed from Wasm SIMD primitives.

use super::*;
use crate::expression::TernaryOp as Ternary;

impl<V: VectorType> Val<V> {
    /// Repeats the scalar's logical bits in every lane of its width.
    pub fn splat<T: VectorLane>(value: impl Into<Val<T>>) -> Self {
        value.into().unary(Unary::Splat { lane_type: T::TYPE })
    }

    /// Selects each output byte from this vector, low byte first.
    /// Indices below the logical byte width select a byte; all others produce zero.
    pub fn swizzle_bytes(&self, indices: impl Into<Self>) -> Self {
        self.binary(
            Binary::Swizzle {
                input_type: V::TYPE,
            },
            indices,
        )
    }

    /// Selects each bit from `when_true` where this mask is one, and from
    /// `when_false` where it is zero. Selection is per bit, not per lane.
    pub fn select_bits(&self, when_true: impl Into<Self>, when_false: impl Into<Self>) -> Self {
        when_true
            .into()
            .ternary(Ternary::Bitselect, when_false, self)
    }

    /// Extracts a lane, numbered from the least significant bits.
    /// The index wraps modulo the number of lanes at the requested width.
    pub fn extract_lane<T: VectorLane>(&self, index: impl Into<Val<I32>>) -> Val<T> {
        let width = T::TYPE.bits();
        let offset = index
            .into()
            .and(u32::from(V::TYPE.bits() / width - 1))
            .shl((width / 8).trailing_zeros());
        let byte_positions = (0..V::TYPE.bits() / 8)
            .fold(0_u128, |bits, byte| bits | (u128::from(byte) << (byte * 8)));
        let positions = Val::<V128>::from(byte_positions).unary::<V>(Unary::Convert);
        let indices = Self::splat(offset.truncate::<I8>()).add_lanes::<I8>(positions);
        self.swizzle_bytes(indices)
            .unary(Unary::ExtractLane { lane: 0 })
    }

    /// Replaces one lane and preserves all other bits.
    /// The index wraps modulo the number of lanes at the supplied value's width.
    pub fn replace_lane<T: VectorLane>(
        &self,
        index: impl Into<Val<I32>>,
        value: impl Into<Val<T>>,
    ) -> Self {
        let width = T::TYPE.bits();
        let index = index
            .into()
            .and(u32::from(V::TYPE.bits() / width - 1))
            .convert::<T>();
        let lane_numbers = (0..128 / width).fold(0_u128, |bits, lane| {
            bits | (u128::from(lane) << (lane * width))
        });
        let mask = Val::<V128>::splat(index)
            .eq_lanes::<T>(lane_numbers)
            .unary::<V>(Unary::Convert);
        mask.select_bits(Self::splat(value), self)
    }
}
