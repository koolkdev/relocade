//! Exact alignment and cancellation precede the shared precision/range rounding.

mod full_precision;
pub(super) use full_precision::full_precision_candidate;

use crate::floating::{BinaryFormat, FiniteInput, RoundingMode};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I8};

use super::{
    operand::{AddendSigns, BinaryOperands, Operand},
    result::{ArithmeticCandidate, BinaryArithmetic, RoundedShape, RoundedValue},
    value::{ConditionalBinary64, KnownClass},
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

/// Start native accumulation with short terms, or reuse the certified binary64
/// arm of a previous sum. Full-width inputs take the general sum directly.
fn native_addend(
    body: &BlockBuilder<'_>,
    operand: &Operand,
) -> Result<Option<ConditionalBinary64>, BuildError> {
    if let Some(native) = &operand.conditional_binary64 {
        return Ok(Some(native.clone()));
    }
    let Some(value) = operand.binary64_value() else {
        return Ok(None);
    };
    let short = operand.binary32_value().is_some()
        || body.constant_bits(operand.bits.significand.and(0xffff_u64))? == Some(0);
    Ok(short.then(|| ConditionalBinary64 {
        valid: true.into(),
        value: value.clone(),
    }))
}

/// Requires PC64/nearest and finite operands. Both branches execute in the JIT.
pub(super) fn short_sum_candidate(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    rounding: &RoundingMode,
) -> Result<Option<ArithmeticCandidate>, BuildError> {
    let (Some(left), Some(right)) = (
        native_addend(body, &operands.left)?,
        native_addend(body, &operands.right)?,
    ) else {
        return Ok(None);
    };
    let inputs_native = left.valid.and(&right.valid);
    let left = &left.value;
    let right = &right.value;
    let sum = left.add(right);
    // Subtracting the larger-magnitude operand recovers the smaller exactly
    // only when their native sum is exact (FastTwoSum). Check both orders.
    let exact = sum.sub(left).eq(right).and(sum.sub(right).eq(left));
    let zero = sum.eq(0.0);
    let bits = sum.to_bits();
    let exponent = BinaryFormat::Binary64.exponent_field(&bits);
    let ordinary = zero.or(exponent.sub(1).unsigned().lt(2046));
    let use_native = inputs_native.and(exact).and(ordinary);
    let joined_input = operands.left.conditional_binary64.is_some()
        || operands.right.conditional_binary64.is_some();
    let (in_range, components) = body.if_value::<(I1, RoundedShape)>(
        &use_native,
        |body| {
            let class = KnownClass::zero().select(&zero, &KnownClass::normal());
            let value = ExtendedValue::from_classified_binary(BinaryFormat::Binary64, bits, class);
            body.yield_((true, RoundedValue::exact(value).components()))
        },
        |mut body| {
            // Fresh binary64 terms cannot exceed the extended exponent range.
            // A previous wide arm may have carried a value outside binary64.
            let wide =
                full_precision_candidate(&mut body, operands, BinaryOperation::Add, rounding)?;
            let valid = if joined_input {
                wide.valid
            } else {
                true.into()
            };
            body.yield_((valid, wide.rounded.components()))
        },
    )?;
    let class = KnownClass::zero().select(&components.0.eq(0_u64), &KnownClass::normal());
    let mut rounded = RoundedValue::from_components(components, class);
    rounded.value.conditional_binary64 = Some(ConditionalBinary64 {
        valid: use_native,
        value: sum,
    });
    Ok(Some(ArithmeticCandidate {
        valid: operands.normal_or_zero().and(in_range),
        rounded,
    }))
}
