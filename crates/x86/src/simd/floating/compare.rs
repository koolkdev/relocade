//! IEEE ordering and operand exceptions, independent of the destination or MXCSR masks.

use super::SimdExceptions;
use crate::floating::BinaryFormat;
use wasm86_compiler::{AtLeast, Val, VectorLane, I1, I32, I64};

/// The integer encoding of one binary32 or binary64 lane.
pub(crate) trait FloatLane: VectorLane {
    const FORMAT: BinaryFormat;
}

impl FloatLane for I32 {
    const FORMAT: BinaryFormat = BinaryFormat::Binary32;
}

impl FloatLane for I64 {
    const FORMAT: BinaryFormat = BinaryFormat::Binary64;
}

pub(crate) struct FloatOperands<T: FloatLane> {
    /// DAZ-normalized encodings retained for exact result selection.
    pub(crate) left: Val<T>,
    pub(crate) right: Val<T>,
    unordered: Val<I1>,
    signaling_nan: Val<I1>,
    denormal: Val<I1>,
}

pub(crate) struct FloatComparison {
    pub(crate) less: Val<I1>,
    pub(crate) equal: Val<I1>,
    pub(crate) greater: Val<I1>,
    pub(crate) unordered: Val<I1>,
    pub(crate) exceptions: SimdExceptions,
}

impl<T: FloatLane> FloatOperands<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn new(left: Val<T>, right: Val<T>, daz: Val<I1>) -> Self {
        let left_bits = left.unsigned().extend::<I64>();
        let right_bits = right.unsigned().extend::<I64>();
        let left_class = T::FORMAT.classification(&left_bits);
        let right_class = T::FORMAT.classification(&right_bits);
        let left_denormal = left_class.denormal();
        let right_denormal = right_class.denormal();
        let unordered = left_class.nan().or(right_class.nan());
        let sign = Val::<I64>::from(T::FORMAT.sign_bit()).truncate::<T>();
        Self {
            // DAZ retains the sign of a subnormal input's zero replacement.
            left: daz.and(&left_denormal).select(left.and(&sign), left),
            right: daz.and(&right_denormal).select(right.and(sign), right),
            signaling_nan: left_class.signaling_nan().or(right_class.signaling_nan()),
            // A NaN response takes precedence over a denormal in the same lane.
            denormal: unordered
                .eq(false)
                .and(daz.eq(false))
                .and(left_denormal.or(right_denormal)),
            unordered,
        }
    }

    pub(crate) fn compare(&self, signal_quiet_nan: Val<I1>) -> FloatComparison {
        let left = T::FORMAT.binary64_value(&self.left.unsigned().extend::<I64>());
        let right = T::FORMAT.binary64_value(&self.right.unsigned().extend::<I64>());
        FloatComparison {
            less: left.lt(&right),
            equal: left.eq(&right),
            greater: left.gt(&right),
            unordered: self.unordered.clone(),
            exceptions: SimdExceptions {
                invalid: self.signaling_nan.or(signal_quiet_nan.and(&self.unordered)),
                denormal: self.denormal.clone(),
                ..SimdExceptions::default()
            },
        }
    }
}
