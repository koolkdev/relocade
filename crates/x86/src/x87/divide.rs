//! Exact radix-2^32 division supplies a floor quotient and rounding evidence.

use wasm86_compiler::{Val, I1, I32, I64, I8};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    rounding::{FiniteMagnitude, RoundingInput},
    BinaryOperation, RoundingMode,
};

const LEADING: u64 = 1 << 63;
const RADIX: u64 = 1 << 32;

pub(super) fn divide(
    operands: &BinaryOperands,
    operation: BinaryOperation,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> BinaryArithmetic {
    let (dividend, divisor) = match operation {
        BinaryOperation::Divide => (&operands.left, &operands.right),
        BinaryOperation::ReverseDivide => (&operands.right, &operands.left),
        _ => unreachable!(),
    };
    let negative = dividend.bits.negative().xor(divisor.bits.negative());
    let zero = RoundedValue::zero(negative.clone());
    let (numerator, numerator_exponent) = dividend.normalized();
    let (denominator, denominator_exponent) = divisor.normalized();
    // Non-numerical operands use a harmless normalized divisor. Their class
    // responses replace the quotient; guest zero division must not trap in Wasm.
    let divisor_digits = Divisor::new(denominator.or(LEADING));
    let below_one = numerator.unsigned().lt(&divisor_digits.significand);
    let magnitude = FiniteMagnitude {
        significand: divisor_digits.quotient(&numerator, &below_one),
        exponent: numerator_exponent
            .sub(denominator_exponent)
            .sub(below_one.unsigned().extend::<I32>()),
        negative: negative.clone(),
    };
    let mut result = magnitude.round(precision, rounding);
    result.replace_when(&dividend.zero.or(&divisor.infinity), &zero);
    result.replace_when(
        &dividend.infinity.or(&divisor.zero),
        &RoundedValue::infinity(negative),
    );
    result.zero_divide = divisor
        .zero
        .and(dividend.zero.or(&dividend.infinity).eq(false));
    let invalid = dividend
        .zero
        .and(&divisor.zero)
        .or(dividend.infinity.and(&divisor.infinity));
    BinaryArithmetic {
        result: operands.finish(result, invalid),
        operands_valid: operands.precision_only(operation),
        round_magnitude: dividend.zero.eq(false),
        zero,
    }
}

/// A normalized two-digit divisor bounds each quotient estimate to at most
/// two above the exact digit. Corrections are pure selections, so value
/// placement can omit the entire calculation on an exact-zero result path.
struct Divisor {
    significand: Val<I64>,
    high: Val<I64>,
    low: Val<I64>,
}

struct QuotientDigit {
    quotient: Val<I64>,
    remainder: Val<I64>,
}

impl Divisor {
    fn new(significand: Val<I64>) -> Self {
        Self {
            high: significand.unsigned().shr(32),
            low: significand.and(RADIX - 1),
            significand,
        }
    }

    fn digit(&self, high: Val<I64>, low: Val<I64>) -> QuotientDigit {
        let mut quotient = high.unsigned().div(&self.high);
        let mut remainder = high.sub(quotient.mul(&self.high));
        for _ in 0..2 {
            // Once the provisional remainder reaches the radix, the digit
            // fits. Testing that bound also keeps the cross-product in u64.
            let too_large = remainder.unsigned().lt(RADIX).and(
                quotient.unsigned().ge(RADIX).or(remainder
                    .shl(32)
                    .or(&low)
                    .unsigned()
                    .lt(quotient.mul(&self.low))),
            );
            quotient = quotient.sub(too_large.unsigned().extend::<I64>());
            remainder = remainder.add(too_large.select(&self.high, 0_u64));
        }
        QuotientDigit {
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
        let complement = self.significand.sub(&lower.remainder);
        RoundingInput {
            integer: upper.quotient.shl(32).or(lower.quotient),
            // Compare r with B-r to avoid overflowing 2*r. At exactly half,
            // guard is set and sticky is clear for ties-to-even rounding.
            guard: lower.remainder.unsigned().ge(&complement),
            sticky: lower
                .remainder
                .ne(0_u64)
                .and(lower.remainder.ne(complement)),
        }
    }
}
