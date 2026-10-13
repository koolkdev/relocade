//! Lane multiplication and widening products share explicit signedness.

use super::*;
use crate::DoubleWidth;

impl<V: VectorType> Val<V> {
    /// Multiplies corresponding I16, I32 or I64 lanes, wrapping at the lane width.
    pub fn mul_lanes<T: VectorLane + AtLeast<I16>>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::Mul, right.into())
    }

    /// Multiplies corresponding signed I16 lanes and adds adjacent products.
    /// Produces one I32 lane per pair, with each sum wrapping at 32 bits.
    pub fn signed_dot_i16_pairs(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<I16>(LaneBinaryOp::DotI16Signed, right.into())
    }
}

impl Val<V64> {
    /// Multiplies all signed T lanes into double-width lanes in a 128-bit result.
    /// T is I8, I16 or I32. No upper carrier bits participate.
    pub fn signed_widening_mul_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Val<V128> {
        self.binary(
            Binary::Vector {
                operator: LaneBinaryOp::WideningMulLow { signed: true },
                lane_type: T::TYPE,
            },
            right,
        )
    }

    /// Multiplies all unsigned T lanes into double-width lanes in a 128-bit result.
    /// T is I8, I16 or I32. No upper carrier bits participate.
    pub fn unsigned_widening_mul_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Val<V128> {
        self.binary(
            Binary::Vector {
                operator: LaneBinaryOp::WideningMulLow { signed: false },
                lane_type: T::TYPE,
            },
            right,
        )
    }
}

impl Val<V128> {
    /// Multiplies signed T lanes in the low 64 bits, producing double-width lanes.
    /// The high halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn signed_widening_mul_low_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::WideningMulLow { signed: true }, right.into())
    }

    /// Multiplies signed T lanes in the high 64 bits, producing double-width lanes.
    /// The low halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn signed_widening_mul_high_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::WideningMulHigh { signed: true }, right.into())
    }

    /// Multiplies unsigned T lanes in the low 64 bits, producing double-width lanes.
    /// The high halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn unsigned_widening_mul_low_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::WideningMulLow { signed: false }, right.into())
    }

    /// Multiplies unsigned T lanes in the high 64 bits, producing double-width lanes.
    /// The low halves of both inputs are ignored. T is I8, I16 or I32.
    pub fn unsigned_widening_mul_high_lanes<T: VectorLane + DoubleWidth>(
        &self,
        right: impl Into<Self>,
    ) -> Self {
        self.binary_lanes::<T>(
            LaneBinaryOp::WideningMulHigh { signed: false },
            right.into(),
        )
    }
}
