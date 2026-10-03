//! Exact significand multiplication and x87 operand-class responses.

use wasm86_compiler::{Val, I32, I8};

use super::{
    operand::BinaryOperands,
    result::{BinaryArithmetic, RoundedValue},
    rounding::{FiniteMagnitude, RoundingInput},
    RoundingMode,
};

pub(super) fn multiply(
    operands: &BinaryOperands,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> BinaryArithmetic {
    const LEADING: u64 = 1 << 63;
    let BinaryOperands { left, right, .. } = operands;
    let zero_product = left.zero.or(&right.zero);
    let negative = left.bits.negative().xor(right.bits.negative());
    let zero = RoundedValue::zero(negative.clone());
    let (left_significand, left_exponent) = left.normalized();
    let (right_significand, right_exponent) = right.normalized();
    let (low, high) = left_significand.unsigned().mul_wide(right_significand);
    let upper_binade = high.and(LEADING).ne(0_u64);
    let integer = upper_binade.select(&high, high.shl(1).or(low.unsigned().shr(63)));
    let fraction = upper_binade.select(&low, low.shl(1));
    let magnitude = FiniteMagnitude {
        significand: RoundingInput {
            integer,
            guard: fraction.and(LEADING).ne(0_u64),
            sticky: fraction.and(LEADING - 1).ne(0_u64),
        },
        exponent: left_exponent
            .add(right_exponent)
            .add(upper_binade.unsigned().extend::<I32>()),
        negative: negative.clone(),
    };
    let mut result = magnitude.round(precision, rounding);
    result.replace_when(&zero_product, &zero);
    let invalid_operation = left.infinity.or(&right.infinity).and(&zero_product);
    BinaryArithmetic {
        result: operands.finish(result, invalid_operation, negative),
        operands_valid: operands.precision_only(),
        round_magnitude: zero_product.eq(false),
        zero,
    }
}
