//! IEEE ordering and operand exceptions, independent of the destination or MXCSR masks.

use super::{FloatOperand, SimdExceptions};
use crate::floating::BinaryEncoding;
use wasm86_compiler::{AtLeast, Val, I1, I64};

pub(crate) struct FloatOperands<T: BinaryEncoding> {
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

impl<T: BinaryEncoding> FloatOperands<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn new(left: Val<T>, right: Val<T>, daz: Val<I1>) -> Self {
        let left = FloatOperand::new(left, daz.clone());
        let right = FloatOperand::new(right, daz);
        let left_class = left.classification();
        let right_class = right.classification();
        let unordered = left_class.nan().or(right_class.nan());
        Self {
            left: left.bits.clone(),
            right: right.bits.clone(),
            signaling_nan: left_class.signaling_nan().or(right_class.signaling_nan()),
            // A NaN response takes precedence over a denormal in the same lane.
            denormal: unordered.eq(false).and(left.denormal.or(&right.denormal)),
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
