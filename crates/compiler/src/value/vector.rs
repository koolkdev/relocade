//! Integer lane operations preserve their logical widths independently.

use crate::expression::{BinaryOp as Binary, ShiftOp as Shift, UnaryOp as Unary};
use crate::{
    expression::LaneBinaryOp, AtLeast, BitwiseType, IntType, Type, Val, I16, I32, I64, I8, V128,
    V64,
};
use crate::{expression::ShuffleMask, integer::ShiftOp};

mod width;

/// A raw vector with a fixed logical byte width, independent of its Wasm carrier.
pub trait VectorType: BitwiseType {
    /// A byte array matching the logical vector width, including shuffle indices.
    type Bytes: AsRef<[u8]> + AsMut<[u8]> + Default;
}

impl VectorType for V64 {
    type Bytes = [u8; 8];
}
impl VectorType for V128 {
    type Bytes = [u8; 16];
}

/// An integer lane width in a vector: I8, I16, I32 or I64.
pub trait VectorLane: IntType {}
impl VectorLane for I8 {}
impl VectorLane for I16 {}
impl VectorLane for I32 {}
impl VectorLane for I64 {}

/// A byte or word lane supporting saturating SIMD operations.
pub trait SaturatingLane: VectorLane {}
impl SaturatingLane for I8 {}
impl SaturatingLane for I16 {}

impl<V: VectorType> Val<V> {
    /// Shifts each lane left by a shared count, modulo the lane width.
    pub fn shl_lanes<T: VectorLane>(&self, count: impl Into<Val<I32>>) -> Self {
        self.shift_lanes::<T>(ShiftOp::Left, count.into())
    }

    /// Shifts each lane right, inserting zeros. The count is modulo the lane width.
    pub fn unsigned_shr_lanes<T: VectorLane>(&self, count: impl Into<Val<I32>>) -> Self {
        self.shift_lanes::<T>(ShiftOp::RightUnsigned, count.into())
    }

    /// Shifts each lane right, inserting sign bits. The count is modulo the lane width.
    pub fn signed_shr_lanes<T: VectorLane>(&self, count: impl Into<Val<I32>>) -> Self {
        self.shift_lanes::<T>(ShiftOp::RightSigned, count.into())
    }

    fn shift_lanes<T: VectorLane>(&self, operator: ShiftOp, count: Val<I32>) -> Self {
        self.shift(
            Shift::Lanes {
                operator,
                lane_type: T::TYPE,
            },
            count,
        )
    }

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

    /// Multiplies corresponding I16, I32 or I64 lanes, wrapping at the lane width.
    pub fn mul_lanes<T: VectorLane + AtLeast<I16>>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::Mul, right.into())
    }

    /// Multiplies corresponding signed I16 lanes and adds adjacent products.
    /// Produces one I32 lane per pair, with each sum wrapping at 32 bits.
    pub fn signed_dot_i16_pairs(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<I16>(LaneBinaryOp::DotI16Signed, right.into())
    }

    /// Compares corresponding integer lanes for equality, returning all-one bits
    /// in each equal lane and zero bits in each unequal lane.
    pub fn eq_lanes<T: VectorLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::Eq, right.into())
    }

    /// Compares corresponding signed integer lanes, returning all-one bits where
    /// this lane is greater than the right lane and zero bits otherwise.
    pub fn signed_gt_lanes<T: VectorLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::GtSigned, right.into())
    }

    /// Adds signed lanes, clamping each result to that lane's range.
    pub fn signed_saturating_add_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::AddSatSigned, right.into())
    }

    /// Subtracts signed lanes, clamping each result to that lane's range.
    pub fn signed_saturating_sub_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::SubSatSigned, right.into())
    }

    /// Adds unsigned lanes, clamping each result to that lane's range.
    pub fn unsigned_saturating_add_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::AddSatUnsigned, right.into())
    }

    /// Subtracts unsigned lanes, clamping each result to that lane's range.
    pub fn unsigned_saturating_sub_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.binary_lanes::<T>(LaneBinaryOp::SubSatUnsigned, right.into())
    }

    /// Narrows signed lanes of twice T's width into signed T lanes, saturating.
    /// The low half contains this input's results; the high half contains `right`'s.
    pub fn signed_narrow_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.narrow_lanes::<T>(right.into(), true)
    }

    /// Narrows signed lanes of twice T's width into unsigned T lanes, saturating.
    /// Negative inputs become zero. Input order is the same as `signed_narrow_lanes`.
    pub fn unsigned_narrow_lanes<T: SaturatingLane>(&self, right: impl Into<Self>) -> Self {
        self.narrow_lanes::<T>(right.into(), false)
    }

    fn narrow_lanes<T: SaturatingLane>(&self, right: Self, signed: bool) -> Self {
        let operator = Binary::Narrow {
            signed,
            result_lane: T::TYPE,
        };
        if V::TYPE == Type::V64 {
            // The canonical narrowing operation consumes full carriers. Arrange
            // both logical inputs in its first carrier before narrowing.
            let joined = self.concat_low_halves(right);
            joined.binary(operator, Val::<V128>::from(0_u128))
        } else {
            self.binary(operator, right)
        }
    }

    fn binary_lanes<T: VectorLane>(&self, operator: LaneBinaryOp, right: Self) -> Self {
        self.binary(
            Binary::Vector {
                operator,
                lane_type: T::TYPE,
            },
            right,
        )
    }

    /// Selects each output byte from the concatenated input vectors.
    /// Indices 0..N address this vector and N..2*N address `right`, low byte first,
    /// where N is 8 for V64 and 16 for V128. Panics for indices outside that range.
    pub fn shuffle_bytes(&self, right: impl Into<Self>, lanes: V::Bytes) -> Self {
        let width = V::TYPE.bits() / 8;
        let lanes = lanes.as_ref();
        let indices = std::array::from_fn(|byte| {
            if byte >= usize::from(width) {
                return 0;
            }
            let lane = lanes[byte];
            assert!(lane < width * 2, "shuffle byte is in range");
            if lane < width {
                lane
            } else {
                lane + 16 - width
            }
        });
        self.binary(Binary::Shuffle(ShuffleMask::new(indices)), right)
    }

    /// Extracts a lane, numbered from the least significant bits.
    /// Panics if `lane` is outside the vector at the requested width.
    pub fn extract_lane<T: VectorLane>(&self, lane: u8) -> Val<T> {
        assert!(
            lane < V::TYPE.bits() / T::TYPE.bits(),
            "vector lane is in range"
        );
        self.unary(Unary::ExtractLane { lane })
    }

    /// Replaces one lane and preserves all other bits.
    /// Panics if `lane` is outside the vector at the supplied value's width.
    pub fn replace_lane<T: VectorLane>(&self, lane: u8, value: impl Into<Val<T>>) -> Self {
        assert!(
            lane < V::TYPE.bits() / T::TYPE.bits(),
            "vector lane is in range"
        );
        self.binary(
            Binary::ReplaceLane {
                lane_type: T::TYPE,
                lane,
            },
            value,
        )
    }
}
