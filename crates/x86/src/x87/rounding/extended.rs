//! Rounds normalized magnitudes to x87 precision and extended exponent range.

use wasm86_compiler::{Val, I1, I16, I32, I64, I8};

use crate::x87::{
    result::{ArithmeticCandidate, ArithmeticResult, RoundedValue},
    ExtendedBits, ExtendedValue,
};

use super::{RoundingInput, RoundingMode};

/// The leading significand bit is bit 63. Its fractional evidence and signed,
/// unbiased exponent still describe the unrounded result.
pub(in crate::x87) struct FiniteMagnitude {
    pub(in crate::x87) significand: RoundingInput,
    pub(in crate::x87) exponent: Val<I32>,
    pub(in crate::x87) negative: Val<I1>,
}

impl FiniteMagnitude {
    pub(in crate::x87) fn round(
        &self,
        precision: Val<I8>,
        rounding: &RoundingMode,
    ) -> ArithmeticResult {
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
            valid: below_normal.or(&overflow).or(&tiny).eq(false),
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
            overflow,
            tiny,
            masked,
            adjusted,
        }
    }
}
