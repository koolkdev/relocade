//! IEEE ordering and operand exceptions, independent of the destination or MXCSR masks.

use super::{FloatOperand, FloatResult, SimdExceptions};
use crate::floating::BinaryEncoding;
use relocade_compiler::{AtLeast, Val, I1, I64, I8};

#[derive(Clone, Copy)]
pub(crate) enum FloatMinMax {
    Min,
    Max,
}

pub(crate) struct FloatOperands<T: BinaryEncoding> {
    /// DAZ-normalized encodings retained for exact result selection.
    left: Val<T>,
    right: Val<T>,
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

    /// Legacy mask predicates select ordered comparisons or their unordered complements.
    pub(crate) fn compare_mask(&self, predicate: Val<I8>) -> FloatResult<T> {
        // Predicates 4..7 negate 0..3; LT/LE and their negations signal on QNaN.
        let kind = predicate.and(3);
        let comparison = self.compare(kind.eq(1).or(kind.eq(2)));
        let matches = kind
            .eq(0)
            .and(&comparison.equal)
            .or(kind.eq(1).and(&comparison.less))
            .or(kind.eq(2).and(comparison.less.or(&comparison.equal)))
            .or(kind.eq(3).and(comparison.unordered));
        FloatResult {
            value: matches.xor(predicate.and(4).ne(0)).select(-1, 0),
            exceptions: comparison.exceptions,
        }
    }

    pub(crate) fn min_max(&self, operation: FloatMinMax) -> FloatResult<T> {
        let comparison = self.compare(true.into());
        let take_left = match operation {
            FloatMinMax::Min => comparison.less,
            FloatMinMax::Max => comparison.greater,
        };
        FloatResult {
            // Ties and NaNs select the source, including its exact SNaN encoding.
            value: take_left.select(&self.left, &self.right),
            exceptions: comparison.exceptions,
        }
    }
}
