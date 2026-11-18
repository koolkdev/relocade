//! Exact significand multiplication and x87 operand-class responses.

use crate::floating::{BinaryFormat, FiniteInput, RoundingMode};
use relocade_compiler::{BlockBuilder, BuildError, Val, I8};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    value::KnownClass,
    ArithmeticCandidate, BinaryOperation, ExtendedValue,
};

/// Two finite binary32 operands need at most 48 significand bits and their
/// product fits the normal binary64 exponent range unless it is zero. At PC53
/// or PC64 the multiply is therefore exact under every rounding control.
pub(super) fn exact_binary32_product(
    body: &BlockBuilder<'_>,
    operands: &BinaryOperands,
) -> Result<Option<ArithmeticCandidate>, BuildError> {
    let (Some(left), Some(right)) = (
        operands.left.binary32_value(),
        operands.right.binary32_value(),
    ) else {
        return Ok(None);
    };
    // A product with at most 48 significant bits leaves the last five binary64
    // fraction bits clear, including for either signed zero.
    let bits = body.assume_bits(left.mul(right).to_bits(), 0x1f, 0)?;
    let zero = operands.left.zero.or(&operands.right.zero);
    let class = KnownClass::zero().select(&zero, &KnownClass::normal());
    Ok(Some(ArithmeticCandidate {
        valid: operands.normal_or_zero(),
        rounded: RoundedValue::exact(ExtendedValue::from_classified_binary(
            BinaryFormat::Binary64,
            bits,
            class,
        )),
    }))
}

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
