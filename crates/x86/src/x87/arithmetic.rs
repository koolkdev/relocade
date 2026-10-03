//! Register arithmetic rounds precision and range from the same exact magnitude.

use wasm86_compiler::{Results, Val, I1, I16, I32, I64, I8};

use super::{
    rounding::RoundingInput, value::Classification, ExtendedBits, ExtendedValue, RoundingMode,
};

/// An extended result and the rounding evidence needed for #P and C1.
#[derive(Clone)]
pub(crate) struct RoundedValue {
    pub(crate) value: ExtendedValue,
    /// Rounding lost information from the exact mathematical result.
    pub(crate) inexact: Val<I1>,
    /// Rounding increased the magnitude above its truncated value.
    pub(crate) incremented: Val<I1>,
}

/// Logical components at a compiler value join: bits and rounding evidence.
pub(super) type RoundedShape = (I64, I16, I1, I1);

/// The rounded bits are usable when `valid` is true. Operand exceptions remain
/// the operation's responsibility; a range candidate alone does not exclude them.
pub(crate) struct ArithmeticCandidate {
    pub(crate) valid: Val<I1>,
    pub(crate) rounded: RoundedValue,
}

impl RoundedValue {
    pub(super) fn components(&self) -> <RoundedShape as Results>::Values {
        let bits = self.value.bits();
        (
            bits.significand,
            bits.sign_exponent,
            self.inexact.clone(),
            self.incremented.clone(),
        )
    }

    /// The caller must establish the class wherever these joined bits are used.
    pub(super) fn from_components(
        components: <RoundedShape as Results>::Values,
        class: Classification,
    ) -> Self {
        let (significand, sign_exponent, inexact, incremented) = components;
        Self {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent,
            })
            .assume_class(class),
            inexact,
            incremented,
        }
    }

    pub(super) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        Self {
            value: self.value.select(condition, &otherwise.value),
            inexact: condition.select(&self.inexact, &otherwise.inexact),
            incremented: condition.select(&self.incremented, &otherwise.incremented),
        }
    }
}

pub(crate) struct ArithmeticResult {
    /// The precision-rounded result before any range response.
    /// Precision loss and its rounding-direction evidence remain observable.
    pub(super) in_range: ArithmeticCandidate,
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(super) masked: RoundedValue,
    pub(super) adjusted: RoundedValue,
}

impl ArithmeticResult {
    /// Builds a response whose only possible arithmetic exception is inexact (#P).
    /// The caller must exclude operand and range exceptions first. The rounding
    /// evidence still controls C1 and the masked or unmasked #P response.
    pub(crate) fn from_rounding(rounded: RoundedValue) -> Self {
        Self {
            in_range: ArithmeticCandidate {
                valid: true.into(),
                rounded: rounded.clone(),
            },
            invalid: false.into(),
            denormal: false.into(),
            overflow: false.into(),
            tiny: false.into(),
            masked: rounded.clone(),
            adjusted: rounded,
        }
    }

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
