//! Division shares normalization, rounding and architectural exception responses.

mod quotient;

use wasm86_compiler::{Val, I32, I8};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    rounding::FiniteMagnitude,
    BinaryOperation, RoundingMode,
};

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
    let quotient = quotient::calculate(
        &numerator,
        denominator,
        divisor.precision53_significand.is_some(),
    );
    let magnitude = FiniteMagnitude {
        significand: quotient.significand,
        exponent: numerator_exponent
            .sub(denominator_exponent)
            .sub(quotient.below_one.unsigned().extend::<I32>()),
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
