//! Register arithmetic rounds precision and range from the same exact magnitude.

use wasm86_compiler::{Val, I1, I16, I32, I64, I8};

use super::{rounding::RoundingInput, ExtendedBits, ExtendedValue, RoundingMode};

pub(crate) struct RoundedValue {
    pub(crate) value: ExtendedValue,
    pub(crate) inexact: Val<I1>,
    pub(crate) incremented: Val<I1>,
}

impl RoundedValue {
    pub(super) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        Self {
            value: self.value.select(condition, &otherwise.value),
            inexact: condition.select(&self.inexact, &otherwise.inexact),
            incremented: condition.select(&self.incremented, &otherwise.incremented),
        }
    }
}

pub(crate) struct ArithmeticResult {
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(super) masked: RoundedValue,
    pub(super) adjusted: RoundedValue,
}

impl ArithmeticResult {
    /// Unmasked register range exceptions commit the precision-rounded value
    /// with an adjusted exponent, retaining that value's precision evidence.
    pub(crate) fn resolve_range(&self, unmasked: &Val<I1>) -> RoundedValue {
        self.adjusted.select(unmasked, &self.masked)
    }
}

/// The leading significand bit is bit 63. Its fractional evidence and signed,
/// unbiased exponent still describe the unrounded result.
pub(super) struct FiniteMagnitude {
    pub(super) significand: RoundingInput,
    pub(super) exponent: Val<I32>,
    pub(super) negative: Val<I1>,
}

impl FiniteMagnitude {
    pub(super) fn round(&self, precision: Val<I8>, rounding: &RoundingMode) -> ArithmeticResult {
        const LEADING: u64 = 1 << 63;
        let precision = precision.and(3);
        // The reserved PC encoding follows the full-precision policy.
        let discarded = precision.eq(0).select(40, precision.eq(2).select(11, 0));
        let rounded = rounding.round(self.significand.shift_right(&discarded), &self.negative);
        let significand = rounded.integer.shl(&discarded);
        let carry = significand.eq(0_u64);
        let exponent = self.exponent.add(carry.unsigned().extend::<I32>());
        let significand = carry.select(LEADING, significand);
        let overflow = exponent.signed().ge(16384);
        let tiny = exponent.signed().lt(-16382);
        let sign = self.negative.select(0x8000_u32, 0_u32);

        // Masked underflow uses the original magnitude on the subnormal grid,
        // never the already precision-rounded significand above.
        let below_normal = self.exponent.signed().lt(-16382);
        let distance = below_normal.select(
            discarded.add(Val::<I32>::from(-16382).sub(&self.exponent)),
            &discarded,
        );
        let stored = rounding.round(self.significand.shift_right(distance), &self.negative);
        let subnormal = stored.integer.shl(&discarded);
        let stored_exponent = below_normal.select(
            subnormal.and(LEADING).ne(0_u64).unsigned().extend::<I32>(),
            exponent.add(16383),
        );
        let to_infinity = rounding.overflow_to_infinity(&self.negative);
        let saturated = to_infinity.select(LEADING, Val::<I64>::from(u64::MAX).shl(discarded));
        let masked = RoundedValue {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand: overflow
                    .select(saturated, below_normal.select(subnormal, &significand)),
                sign_exponent: sign
                    .or(overflow.select(to_infinity.select(0x7fff, 0x7ffe), stored_exponent))
                    .truncate::<I16>(),
            }),
            inexact: overflow.or(stored.inexact),
            incremented: overflow.select(to_infinity, stored.incremented),
        };
        let adjustment = overflow.select(-24576, tiny.select(24576, 0));
        let adjusted = RoundedValue {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent: sign
                    .or(exponent.add(16383).add(adjustment))
                    .truncate::<I16>(),
            }),
            inexact: rounded.inexact,
            incremented: rounded.incremented,
        };
        ArithmeticResult {
            invalid: false.into(),
            denormal: false.into(),
            overflow,
            tiny,
            masked,
            adjusted,
        }
    }
}
