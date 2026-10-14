//! Exact alignment and cancellation precede the shared precision/range rounding.

use crate::floating::{FiniteInput, RoundingMode};
use wasm86_compiler::{BlockBuilder, BuildError, Val, I8};

use super::{
    operand::{AddendSigns, BinaryOperands},
    result::{BinaryArithmetic, RoundedValue},
    BinaryOperation,
};

pub(super) fn add(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    operation: BinaryOperation,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> Result<BinaryArithmetic, BuildError> {
    let BinaryOperands { left, right, .. } = operands;
    let AddendSigns {
        left_negative,
        right_negative,
    } = operands.addend_signs(operation);
    let subtract_magnitudes = left_negative.xor(&right_negative);
    let (significand, exponent) = left.normalized();
    let left_input = FiniteInput {
        significand,
        exponent,
        negative: left_negative.clone(),
    };
    let (significand, exponent) = right.normalized();
    let right_input = FiniteInput {
        significand,
        exponent,
        negative: right_negative.clone(),
    };
    let magnitude = left_input.add(body, &right_input, rounding)?;
    let exact_zero = magnitude.significand.integer.eq(0_u64);
    let zero = RoundedValue::zero(
        subtract_magnitudes.select(rounding.cancellation_negative(), left_negative.clone()),
    );
    let mut result = magnitude.round(precision, rounding);
    result.replace_when(&exact_zero, &zero);
    let infinity_negative = left.infinity.select(left_negative, right_negative);
    result.replace_when(
        &left.infinity.or(&right.infinity),
        &RoundedValue::infinity(infinity_negative),
    );
    let invalid_operation = left.infinity.and(&right.infinity).and(subtract_magnitudes);
    Ok(BinaryArithmetic {
        result: operands.finish(result, invalid_operation),
        operands_valid: operands.precision_only(operation),
        round_magnitude: exact_zero.eq(false),
        zero,
    })
}
