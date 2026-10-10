//! Integer lanes are bit views of a vector, without numerical conversion.

use crate::{Expression, IntType, Val, I32, I64, V128};

/// A supported integer lane width in a 128-bit vector: I32 or I64.
pub trait VectorLane: IntType {}
impl VectorLane for I32 {}
impl VectorLane for I64 {}

impl Val<V128> {
    /// Extracts a lane, numbered from the least significant bits.
    /// Panics if `lane` is outside the vector at the requested width.
    pub fn extract_lane<T: VectorLane>(&self, lane: u8) -> Val<T> {
        assert!(lane < 128 / T::TYPE.bits(), "vector lane is in range");
        Val::expression(Expression::VectorExtract {
            input: self.into(),
            lane,
        })
    }

    /// Replaces one lane and preserves all other bits.
    /// Panics if `lane` is outside the vector at the supplied value's width.
    pub fn replace_lane<T: VectorLane>(&self, lane: u8, value: impl Into<Val<T>>) -> Self {
        assert!(lane < 128 / T::TYPE.bits(), "vector lane is in range");
        Self::expression(Expression::VectorReplace {
            vector: self.into(),
            value: value.into().into(),
            lane,
        })
    }
}
