//! Explicit changes of vector width and access to a V64's raw scalar encoding.

use super::*;
use crate::DoubleWidth;

impl<V: VectorType> Val<V> {
    pub(super) fn concat_low_halves(&self, high: Self) -> Val<V128> {
        self.binary(
            Binary::ShuffleImmediate(ShuffleMask::new(std::array::from_fn(|byte| {
                if byte < 8 {
                    byte as u8
                } else {
                    byte as u8 + 8
                }
            }))),
            high,
        )
    }
}

impl Val<V64> {
    /// Interprets all 64 integer bits as a vector. Upper carrier bits are unspecified.
    pub fn from_bits(bits: impl Into<Val<I64>>) -> Self {
        bits.into().unary(Unary::Reinterpret)
    }

    /// Returns the vector's complete 64-bit logical encoding.
    pub fn to_bits(&self) -> Val<I64> {
        self.unary(Unary::Reinterpret)
    }

    /// Widens to 128 bits, filling the upper half with zeros.
    pub fn zero_extend(&self) -> Val<V128> {
        self.unary(Unary::Convert)
    }

    /// Places this vector in the low half and `high` in the high half.
    pub fn concat(&self, high: impl Into<Self>) -> Val<V128> {
        self.concat_low_halves(high.into())
    }

    /// Multiplies all signed T lanes into double-width lanes in a 128-bit result.
    /// T is I8, I16 or I32. No upper carrier bits participate.
    pub fn signed_widening_mul_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Val<V128> {
        self.binary(
            Binary::Vector {
                operator: LaneBinaryOp::WideningMulLowSigned,
                lane_type: T::TYPE,
            },
            right,
        )
    }
}

impl Val<V128> {
    /// Keeps the low 64 logical bits. Upper carrier bits remain unobserved.
    pub fn low_half(&self) -> Val<V64> {
        self.unary(Unary::Convert)
    }

    /// Multiplies signed T lanes in the low 64 bits, producing double-width lanes.
    /// The high halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn signed_widening_mul_low_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::WideningMulLowSigned, right.into())
    }

    /// Multiplies signed T lanes in the high 64 bits, producing double-width lanes.
    /// The low halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn signed_widening_mul_high_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::WideningMulHighSigned, right.into())
    }
}
