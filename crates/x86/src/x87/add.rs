//! Exact alignment and cancellation precede the shared precision/range rounding.

mod full_precision;
pub(super) use full_precision::full_precision_candidate;

use crate::floating::{BinaryFormat, FiniteInput, RoundingMode};
use relocade_compiler::{BlockBuilder, BuildError, Val, I8};

use super::{
    operand::{AddendSigns, BinaryOperands, Operand},
    result::{ArithmeticCandidate, BinaryArithmetic, RoundedShape, RoundedValue},
    value::KnownClass,
    BinaryOperation, ExtendedValue,
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

/// Two 48-bit terms leave room for exponent alignment and a carry in binary64.
/// Wider terms use the general sum directly.
fn at_most_48_bits(body: &BlockBuilder<'_>, operand: &Operand) -> Result<bool, BuildError> {
    Ok(operand.binary32_value().is_some()
        || body.constant_bits(operand.bits.significand.and(0xffff_u64))? == Some(0))
}

/// Requires PC64/nearest and finite operands. Both branches execute in the JIT.
pub(super) fn short_sum_candidate(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    rounding: &RoundingMode,
) -> Result<Option<ArithmeticCandidate>, BuildError> {
    let (Some(left), Some(right)) = (
        operands.left.binary64_value(),
        operands.right.binary64_value(),
    ) else {
        return Ok(None);
    };
    if !at_most_48_bits(body, &operands.left)? || !at_most_48_bits(body, &operands.right)? {
        return Ok(None);
    }
    let sum = left.add(right);
    // Subtracting the larger-magnitude operand recovers the smaller exactly
    // only when their native sum is exact (FastTwoSum). Check both orders.
    let exact = sum.sub(left).eq(right).and(sum.sub(right).eq(left));
    let zero = sum.eq(0.0);
    let bits = sum.to_bits();
    let exponent = BinaryFormat::Binary64.exponent_field(&bits);
    let ordinary = zero.or(exponent.sub(1).unsigned().lt(2046));
    let components = body.if_value::<RoundedShape>(
        exact.and(ordinary),
        |body| {
            let class = KnownClass::zero().select(&zero, &KnownClass::normal());
            let value = ExtendedValue::from_classified_binary(BinaryFormat::Binary64, bits, class);
            body.yield_(RoundedValue::exact(value).components())
        },
        |mut body| {
            // A nonzero sum of two finite binary64 values is always in the
            // extended normal range. Exact cancellations took the first arm.
            let wide =
                full_precision_candidate(&mut body, operands, BinaryOperation::Add, rounding)?;
            body.yield_(wide.rounded.components())
        },
    )?;
    let class = KnownClass::zero().select(&components.0.eq(0_u64), &KnownClass::normal());
    Ok(Some(ArithmeticCandidate {
        valid: operands.normal_or_zero(),
        rounded: RoundedValue::from_components(components, class),
    }))
}
