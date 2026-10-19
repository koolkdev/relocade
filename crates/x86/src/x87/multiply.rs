//! Exact significand multiplication and x87 operand-class responses.

use crate::floating::{FiniteInput, RoundingMode};
use relocade_compiler::{Val, I8};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    BinaryOperation,
};

pub(super) fn multiply(
    operands: &BinaryOperands,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> BinaryArithmetic {
    let BinaryOperands { left, right, .. } = operands;
    let zero_product = left.zero.or(&right.zero);
    let negative = left.bits.negative().xor(right.bits.negative());
    let zero = RoundedValue::zero(negative.clone());
    let (left_significand, left_exponent) = left.normalized();
    let (right_significand, right_exponent) = right.normalized();
    let magnitude = FiniteInput {
        significand: left_significand,
        exponent: left_exponent,
        negative: left.bits.negative(),
    }
    .multiply(&FiniteInput {
        significand: right_significand,
        exponent: right_exponent,
        negative: right.bits.negative(),
    });
    let mut result = magnitude.round(precision, rounding);
    result.replace_when(&zero_product, &zero);
    result.replace_when(
        &left.infinity.or(&right.infinity),
        &RoundedValue::infinity(negative),
    );
    let invalid_operation = left.infinity.or(&right.infinity).and(&zero_product);
    BinaryArithmetic {
        result: operands.finish(result, invalid_operation),
        operands_valid: operands.precision_only(BinaryOperation::Multiply),
        round_magnitude: zero_product.eq(false),
        zero,
    }
}
