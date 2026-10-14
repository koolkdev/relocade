//! Normalized quotient algorithms preserve an exact floor and remainder.

use wasm86_compiler::{BlockBuilder, BuildError, Val, F64, I1, I32, I64};

use crate::floating::RoundingInput;

#[cfg(test)]
mod tests;

const LEADING: u64 = 1 << 63;
const RADIX: u64 = 1 << 32;

pub(super) struct Quotient {
    pub(super) significand: RoundingInput,
    pub(super) below_one: Val<I1>,
}

/// Finite nonzero operands have normalized significands. Other classes retain
/// a safe unused calculation until the caller replaces their numerical result.
/// `divisor_precision53` proves the normalized denominator's low eleven bits clear.
pub(super) fn calculate(
    numerator: &Val<I64>,
    denominator: Val<I64>,
    divisor_precision53: bool,
) -> Quotient {
    // Class responses replace non-numerical results. A harmless normalized
    // divisor keeps their unused calculation free of Wasm traps.
    let denominator = denominator.or(LEADING);
    let below_one = numerator.unsigned().lt(&denominator);
    let significand = if divisor_precision53 {
        narrow(numerator, &denominator, &below_one)
    } else {
        Divisor::new(denominator).quotient(numerator, &below_one)
    };
    Quotient {
        significand,
        below_one,
    }
}

/// Requires the normalized denominator's low eleven bits to be zero.
fn narrow(numerator: &Val<I64>, denominator: &Val<I64>, below_one: &Val<I1>) -> RoundingInput {
    let prefix = native_prefix(numerator, denominator, below_one);
    // The remaining eleven quotient bits fit in one integer divide because
    // remainder < divisor < 2^53. This remainder is measured against divisor,
    // not against the original left-aligned extended significand.
    let numerator = prefix.remainder.shl(11);
    let low = numerator.unsigned().div(&prefix.divisor);
    rounding_input(
        prefix.integer.shl(11).or(&low),
        numerator.sub(low.mul(&prefix.divisor)),
        &prefix.divisor,
    )
}

/// The first 53 bits of the normalized quotient and its exact residual, measured
/// against the divisor after removing its eleven trailing zero bits.
struct NativePrefix {
    integer: Val<I64>,
    remainder: Val<I64>,
    divisor: Val<I64>,
}

fn native_prefix(
    numerator: &Val<I64>,
    denominator: &Val<I64>,
    below_one: &Val<I1>,
) -> NativePrefix {
    let divisor = denominator.unsigned().shr(11);
    // Removing the numerator's tail cannot change numerator < denominator.
    // The 53-bit head ratio cannot round across 1 or 2, so its significand
    // uses the same normalization as below_one.
    let head = |integer: &Val<I64>| Val::<F64>::from_bits(integer.or(1023_u64 << 52));
    let estimate = head(&numerator.unsigned().shr(11)).div(head(&divisor));
    let quotient = estimate.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52);
    // The omitted numerator tail increases this quotient by less than two,
    // and native rounding changes it by less than one half. Thus
    // -divisor/2 < remainder < 5*divisor/2: its signed low word is exact.
    let remainder = numerator
        .shl(below_one.select(42, 41))
        .sub(quotient.mul(&divisor));
    let negative = remainder.signed().lt(0);
    let mut quotient = quotient.sub(negative.unsigned().extend::<I64>());
    let mut remainder = remainder.add(negative.select(&divisor, 0_u64));
    // One decrement or at most two increments restore a floor/remainder pair.
    for _ in 0..2 {
        let increment = remainder.unsigned().ge(&divisor);
        quotient = quotient.add(increment.unsigned().extend::<I64>());
        remainder = remainder.sub(increment.select(&divisor, 0_u64));
    }
    NativePrefix {
        integer: quotient,
        remainder,
        divisor,
    }
}

/// Divides `numerator << shift` for normalized numerator and denominator, where
/// 0 <= shift <= 63. The quotient and remainder are exact. A known 53-bit denominator uses
/// the same corrected native estimate as ordinary division.
pub(crate) fn integer_division(
    body: &mut BlockBuilder<'_>,
    numerator: &Val<I64>,
    denominator: Val<I64>,
    shift: &Val<I32>,
    denominator_precision53: bool,
) -> Result<IntegerDivision, BuildError> {
    let denominator = denominator.or(LEADING);
    let quotient = if denominator_precision53 {
        let below_one = numerator.unsigned().lt(&denominator);
        let prefix = native_prefix(numerator, &denominator, &below_one);
        let distance = below_one.select::<I32>(53, 52).sub(shift);
        body.if_value::<I64>(
            distance.signed().ge(0),
            |body| body.yield_(prefix.integer.unsigned().shr(&distance)),
            |body| {
                // Only large quotients need the final one to eleven bits.
                let extra = Val::<I32>::from(0).sub(&distance);
                let low = prefix.remainder.shl(&extra).unsigned().div(&prefix.divisor);
                body.yield_(prefix.integer.shl(extra).or(low))
            },
        )?
    } else {
        let divisor = Divisor::new(denominator.clone());
        let high = shift.eq(0).select(
            0_u64,
            numerator.unsigned().shr(Val::<I32>::from(64).sub(shift)),
        );
        let low = numerator.shl(shift);
        let upper = divisor.digit(high, low.unsigned().shr(32));
        let lower = divisor.digit(upper.remainder, low.and(RADIX - 1));
        upper.quotient.shl(32).or(lower.quotient)
    };
    // The exact floor bounds the residual below the 64-bit denominator, so
    // its low word is the complete residual even when the dividend is wider.
    Ok(IntegerDivision {
        remainder: numerator.shl(shift).sub(quotient.mul(&denominator)),
        quotient,
    })
}

fn rounding_input(integer: Val<I64>, remainder: Val<I64>, divisor: &Val<I64>) -> RoundingInput {
    let complement = divisor.sub(&remainder);
    RoundingInput {
        integer,
        // Compare r with B-r to avoid overflowing 2*r. At exactly half,
        // guard is set and sticky is clear for ties-to-even rounding.
        guard: remainder.unsigned().ge(&complement),
        sticky: remainder.ne(0_u64).and(remainder.ne(complement)),
    }
}

/// A normalized two-digit divisor. Calculations remain pure values, so placement
/// can omit the entire division on an exact-zero result path.
struct Divisor {
    significand: Val<I64>,
    high: Val<I64>,
    low: Val<I64>,
}

pub(crate) struct IntegerDivision {
    pub(crate) quotient: Val<I64>,
    pub(crate) remainder: Val<I64>,
}

impl Divisor {
    fn new(significand: Val<I64>) -> Self {
        Self {
            high: significand.unsigned().shr(32),
            low: significand.and(RADIX - 1),
            significand,
        }
    }

    /// Divides `high * RADIX + low`, requiring `high < self.significand` and
    /// `low < RADIX`. The returned remainder preserves the first bound.
    fn digit(&self, high: Val<I64>, low: Val<I64>) -> IntegerDivision {
        // Normalization gives self.high >= RADIX/2, bounding the estimate by
        // RADIX+1. Even its product with self.low fits in u64.
        let estimate = high.unsigned().div(&self.high);
        let high_remainder = high.sub(estimate.mul(&self.high));
        let low_product = estimate.mul(&self.low);
        let partial_dividend = high_remainder.shl(32).or(&low);
        let excess = low_product.sub(&partial_dividend);
        let too_large = partial_dividend.unsigned().lt(&low_product);
        // A positive excess needs one decrement, or two when it exceeds the
        // divisor. Equality needs only one; the excess is strictly below twice
        // the divisor because it fits in u64 and the divisor is normalized.
        let needs_second = too_large.and(self.significand.unsigned().lt(&excess));
        let quotient = estimate
            .sub(too_large.unsigned().extend::<I64>())
            .sub(needs_second.unsigned().extend::<I64>());
        IntegerDivision {
            remainder: high.shl(32).or(low).sub(quotient.mul(&self.significand)),
            quotient,
        }
    }

    fn quotient(&self, numerator: &Val<I64>, below_one: &Val<I1>) -> RoundingInput {
        // N = numerator << (63 + below_one). Split N into its high word and
        // two radix digits. The high word is strictly less than the divisor.
        let high = below_one.select(numerator, numerator.unsigned().shr(1));
        let low = below_one.select(0_u64, numerator.and(1_u64).shl(31));
        let upper = self.digit(high, low);
        let lower = self.digit(upper.remainder, 0_u64.into());
        rounding_input(
            upper.quotient.shl(32).or(lower.quotient),
            lower.remainder,
            &self.significand,
        )
    }
}
