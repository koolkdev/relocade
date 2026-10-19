//! Packed comparisons and extrema share NaN precedence and operand exceptions.

use super::*;
use crate::simd::FloatMinMax;
use relocade_compiler::I8;

impl PackedFloatOperand {
    /// Negated legacy predicates include unordered pairs. Exception masks are
    /// reduced only after NaN precedence has been applied separately per lane.
    pub(crate) fn compare_mask(&self, right: &Self, predicate: Val<I8>) -> FloatResult<V128> {
        let kind = predicate.and(3);
        let unordered = self.nan().or(right.nan());
        let (equal, less, less_equal) = match self.format {
            BinaryFormat::Binary32 => (
                self.bits.f32x4().eq(&right.bits),
                self.bits.f32x4().lt(&right.bits),
                self.bits.f32x4().le(&right.bits),
            ),
            BinaryFormat::Binary64 => (
                self.bits.f64x2().eq(&right.bits),
                self.bits.f64x2().lt(&right.bits),
                self.bits.f64x2().le(&right.bits),
            ),
        };
        let matches = kind.eq(0).select(
            equal,
            kind.eq(1)
                .select(less, kind.eq(2).select(less_equal, &unordered)),
        );
        FloatResult {
            value: predicate
                .and(4)
                .ne(0)
                .select(matches.xor(u128::MAX), matches),
            exceptions: self.comparison_exceptions(right, kind.eq(1).or(kind.eq(2))),
        }
    }

    pub(crate) fn min_max(&self, right: &Self, operation: FloatMinMax) -> FloatResult<V128> {
        let take_left = match (self.format, operation) {
            (BinaryFormat::Binary32, FloatMinMax::Min) => self.bits.f32x4().lt(&right.bits),
            (BinaryFormat::Binary32, FloatMinMax::Max) => self.bits.f32x4().gt(&right.bits),
            (BinaryFormat::Binary64, FloatMinMax::Min) => self.bits.f64x2().lt(&right.bits),
            (BinaryFormat::Binary64, FloatMinMax::Max) => self.bits.f64x2().gt(&right.bits),
        };
        FloatResult {
            // Ordered comparisons leave ties and NaNs to the source. Select raw
            // encodings so signed zero and signaling NaN payloads survive.
            value: take_left.select_bits(&self.bits, &right.bits),
            exceptions: self.comparison_exceptions(right, true.into()),
        }
    }

    fn comparison_exceptions(&self, right: &Self, signal_quiet_nan: Val<I1>) -> SimdExceptions {
        let unordered = self.nan().or(right.nan());
        let signaling = self.signaling_nan().or(right.signaling_nan());
        SimdExceptions {
            invalid: signal_quiet_nan
                .select(&unordered, signaling)
                .bitmask::<I32>()
                .ne(0),
            denormal: self
                .denormal
                .or(&right.denormal)
                .and(unordered.xor(u128::MAX))
                .bitmask::<I32>()
                .ne(0),
            ..SimdExceptions::default()
        }
    }
}
