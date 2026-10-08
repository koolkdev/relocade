//! Comparisons order exact extended values without precision or rounding control.

use wasm86_compiler::{Val, I1};

use super::BinaryOperands;

#[derive(Clone, Copy)]
pub(crate) enum ComparisonKind {
    /// FCOM and FTST signal invalid for every NaN.
    Ordered,
    /// FUCOM accepts quiet NaNs but still signals invalid for signaling NaNs.
    Unordered,
}

pub(crate) struct ComparisonResult {
    // Ordering predicates apply when `unordered` is false.
    pub(crate) less: Val<I1>,
    pub(crate) equal: Val<I1>,
    pub(crate) unordered: Val<I1>,
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
}

impl BinaryOperands {
    pub(crate) fn compare(&self, kind: ComparisonKind) -> ComparisonResult {
        let Self { left, right, .. } = self;
        let (less, equal) = self.ordering();
        let unsupported = left.unsupported.or(&right.unsupported);
        let nan = left.nan.or(&right.nan);
        let unordered = unsupported.or(&nan);
        let invalid = unsupported.or(match kind {
            ComparisonKind::Ordered => nan,
            ComparisonKind::Unordered => left.signaling_nan.or(&right.signaling_nan),
        });
        ComparisonResult {
            less,
            equal,
            denormal: unordered.eq(false).and(left.denormal.or(&right.denormal)),
            unordered,
            invalid,
        }
    }

    fn ordering(&self) -> (Val<I1>, Val<I1>) {
        let Self { left, right, .. } = self;
        if let (Some(left), Some(right)) = (&left.binary64_value, &right.binary64_value) {
            return (left.lt(right), left.eq(right));
        }
        let left_exponent = left.bits.exponent_field();
        let right_exponent = right.bits.exponent_field();
        // E=0 and E=1 have the same scale. This also compares a pseudo-denormal
        // with its normal encoding as equal, while retaining its #D evidence.
        let left_exponent = left_exponent.eq(0).select(1_u32, &left_exponent);
        let right_exponent = right_exponent.eq(0).select(1_u32, &right_exponent);
        let same_exponent = left_exponent.eq(&right_exponent);
        let same_magnitude = same_exponent.and(left.bits.significand.eq(&right.bits.significand));
        let magnitude_less = left_exponent
            .unsigned()
            .lt(&right_exponent)
            .or(same_exponent.and(left.bits.significand.unsigned().lt(&right.bits.significand)));
        let left_negative = left.bits.negative();
        let right_negative = right.bits.negative();
        let both_zero = left.zero.and(&right.zero);
        let equal = both_zero.or(same_magnitude.and(left_negative.eq(&right_negative)));
        let less = both_zero
            .eq(false)
            .and(left_negative.ne(&right_negative).select(
                &left_negative,
                left_negative.select(magnitude_less.or(&same_magnitude).eq(false), magnitude_less),
            ));
        (less, equal)
    }
}
