//! Integer lanes preserve their widths and wrap arithmetic independently.

use crate::{expression::LaneBinaryOp, Expression, IntType, Val, I16, I32, I64, I8, V128};

/// An integer lane width in a 128-bit vector: I8, I16, I32 or I64.
pub trait VectorLane: IntType {}
impl VectorLane for I8 {}
impl VectorLane for I16 {}
impl VectorLane for I32 {}
impl VectorLane for I64 {}

impl Val<V128> {
    /// Adds corresponding integer lanes, wrapping at each lane's logical width.
    /// Carries never cross a lane boundary.
    pub fn add_lanes<T: VectorLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::Add, right.into())
    }

    /// Subtracts corresponding integer lanes, wrapping at each lane's logical width.
    /// Borrows never cross a lane boundary.
    pub fn sub_lanes<T: VectorLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::Sub, right.into())
    }

    fn binary_lanes<T: VectorLane>(&self, operator: LaneBinaryOp, right: Self) -> Self {
        Self::expression(Expression::VectorBinary {
            operator,
            lane_type: T::TYPE,
            left: self.into(),
            right: right.into(),
        })
    }

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
