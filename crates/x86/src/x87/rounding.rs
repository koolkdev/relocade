//! Rounds normalized magnitudes to x87 precision and extended exponent range.

use wasm86_compiler::{Results, Val, I1, I16, I32, I64, I8};

use super::{
    result::{ArithmeticCandidate, ArithmeticResult, RoundedValue},
    ExtendedBits, ExtendedValue,
};

use crate::floating::{RoundingInput, RoundingMode};

/// The leading significand bit is bit 63. Its fractional evidence and signed,
/// unbiased exponent still describe the unrounded result.
pub(super) struct FiniteMagnitude {
    pub(super) significand: RoundingInput,
    pub(super) exponent: Val<I32>,
    pub(super) negative: Val<I1>,
}

pub(super) type MagnitudeShape = (I64, I1, I1, I32, I1);

impl FiniteMagnitude {
    pub(super) fn components(&self) -> <MagnitudeShape as Results>::Values {
        (
            self.significand.integer.clone(),
            self.significand.guard.clone(),
            self.significand.sticky.clone(),
            self.exponent.clone(),
            self.negative.clone(),
        )
    }

    pub(super) fn from_components(
        (integer, guard, sticky, exponent, negative): <MagnitudeShape as Results>::Values,
    ) -> Self {
        Self {
            significand: RoundingInput {
                integer,
                guard,
                sticky,
            },
            exponent,
            negative,
        }
    }

    /// Records a negative tail too small to cross a nonzero remainder boundary.
    /// The supplied rounding evidence must describe the value before that tail exactly.
    /// At an integer or half-integer boundary it changes the rounding evidence;
    /// at other fractions it only records inexactness. A borrow can cross a binade.
    pub(super) fn with_negative_tail(mut self, tail: &Val<I1>) -> Self {
        let boundary = tail.and(self.significand.sticky.eq(false));
        let borrow = boundary.and(self.significand.guard.eq(false));
        let lower_binade = borrow.and(self.significand.integer.eq(1_u64 << 63));
        self.significand.integer = lower_binade.select(
            u64::MAX,
            self.significand
                .integer
                .sub(borrow.unsigned().extend::<I64>()),
        );
        self.significand.guard =
            boundary.select(self.significand.guard.eq(false), &self.significand.guard);
        self.significand.sticky = tail.or(&self.significand.sticky);
        self.exponent = self.exponent.sub(lower_binade.unsigned().extend::<I32>());
        self
    }

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
        let below_normal = self.exponent.signed().lt(-16382);
        let in_range = ArithmeticCandidate {
            // Rounding can only keep or increase the exponent, so the lower
            // bound before rounding also proves it afterwards.
            valid: below_normal.or(&overflow).eq(false),
            rounded: RoundedValue {
                value: ExtendedValue::from_bits(ExtendedBits {
                    significand: significand.clone(),
                    sign_exponent: sign.or(exponent.add(16383)).truncate::<I16>(),
                }),
                inexact: rounded.inexact.clone(),
                incremented: rounded.incremented.clone(),
            },
        };

        // Masked underflow uses the original magnitude on the subnormal grid,
        // never the already precision-rounded significand above.
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
            in_range,
            invalid: false.into(),
            denormal: false.into(),
            zero_divide: false.into(),
            overflow,
            tiny,
            masked,
            adjusted,
        }
    }
}
