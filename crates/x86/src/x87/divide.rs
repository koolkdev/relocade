//! Division shares normalization, rounding and architectural exception responses.

use relocade_compiler::{BlockBuilder, BuildError, Val, I8};

use crate::floating::{FiniteInput, QuotientMethod, RoundingMode};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    BinaryOperation,
};

pub(super) fn divide(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    operation: BinaryOperation,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> Result<BinaryArithmetic, BuildError> {
    let (dividend, divisor) = match operation {
        BinaryOperation::Divide => (&operands.left, &operands.right),
        BinaryOperation::ReverseDivide => (&operands.right, &operands.left),
        _ => unreachable!(),
    };
    let negative = dividend.bits.negative().xor(divisor.bits.negative());
    let zero = RoundedValue::zero(negative.clone());
    let (numerator, numerator_exponent) = dividend.normalized();
    let (denominator, denominator_exponent) = divisor.normalized();
    let method = if divisor.precision53_significand.is_some() {
        QuotientMethod::DivisorPrecision53
    } else if body.constant_bits(precision.and(3))? == Some(0) {
        // The low bit of the 32-bit intermediate lies below the PC24 guard.
        // Jamming any tail there preserves ties, inexactness and rounding direction.
        QuotientMethod::RoundToOdd32
    } else {
        QuotientMethod::Full
    };
    let magnitude = FiniteInput {
        significand: numerator,
        exponent: numerator_exponent,
        negative: dividend.bits.negative(),
    }
    .divide(
        &FiniteInput {
            significand: denominator,
            exponent: denominator_exponent,
            negative: divisor.bits.negative(),
        },
        method,
    );
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
    Ok(BinaryArithmetic {
        result: operands.finish(result, invalid),
        operands_valid: operands.precision_only(operation),
        round_magnitude: dividend.zero.eq(false),
        zero,
    })
}
