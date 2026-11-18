//! Division shares normalization, rounding and architectural exception responses.

use relocade_compiler::{BlockBuilder, BuildError, Val, I8};

use crate::floating::{FiniteInput, FiniteMagnitude, MagnitudeShape, QuotientMethod, RoundingMode};

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
    let numerator = FiniteInput {
        significand: numerator,
        exponent: numerator_exponent,
        negative: dividend.bits.negative(),
    };
    let denominator = FiniteInput {
        significand: denominator,
        exponent: denominator_exponent,
        negative: divisor.bits.negative(),
    };
    let magnitude = finite_quotient(body, &numerator, &denominator, method)?;
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

fn finite_quotient(
    body: &mut BlockBuilder<'_>,
    numerator: &FiniteInput,
    denominator: &FiniteInput,
    method: QuotientMethod,
) -> Result<FiniteMagnitude, BuildError> {
    let power_of_two = denominator.significand.eq(1_u64 << 63);
    let scaled = || {
        FiniteInput {
            significand: numerator.significand.clone(),
            exponent: numerator.exponent.sub(&denominator.exponent),
            negative: numerator.negative.xor(&denominator.negative),
        }
        .magnitude()
    };
    let divide = || numerator.divide(denominator, method);
    match body.constant_bits(&power_of_two)? {
        Some(1) => Ok(scaled()),
        // Only the full-width method needs both radix-32 quotient digits.
        // Its runtime shortcut also keeps a zero numerator out of the divider.
        None if matches!(method, QuotientMethod::Full) => {
            let components = body.if_value::<MagnitudeShape>(
                power_of_two.or(numerator.significand.eq(0_u64)),
                |body| body.yield_(scaled().components()),
                |body| body.yield_(divide().components()),
            )?;
            Ok(FiniteMagnitude::from_components(components))
        }
        _ => Ok(divide()),
    }
}
