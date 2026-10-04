//! PC53/nearest arithmetic retains native significands and x87's wider exponent range.

use wasm86_compiler::{Val, F64, I16, I32, I64};

use super::{
    operand::BinaryOperands,
    result::RoundedValue,
    value::{Classification, Precision53},
    ArithmeticCandidate, BinaryOperation, ExtendedValue,
};

fn integer_significand(value: &Val<F64>) -> Val<I64> {
    value.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52)
}

/// Requires PC53/nearest and the operands' exact normalized native significands.
pub(super) fn multiply(
    operands: &BinaryOperands,
    left: &Val<F64>,
    right: &Val<F64>,
) -> ArithmeticCandidate {
    // Nonzero coefficients are in [1, 2); their product cannot overflow or
    // underflow binary64. Scaling by one half is exact at this magnitude.
    let product = left.mul(right);
    let carry = product.ge(2.0);
    let normalized = carry.select(product.mul(0.5), &product);
    let zero = operands.left.zero.or(&operands.right.zero);
    let nonzero = zero.eq(false);
    let exponent = operands
        .left
        .bits
        .exponent_field()
        .add(operands.right.bits.exponent_field())
        .sub(16383)
        .add(carry.unsigned().extend::<I32>());
    // The exact residual A*B - C*2^shift fits in signed 54 bits. Its low word
    // gives both exactness and magnitude increment without a wide product.
    let residual = integer_significand(left)
        .mul(integer_significand(right))
        .sub(integer_significand(&product).shl(carry.select(53, 52)));
    let incremented = nonzero.and(residual.signed().lt(0));
    // The lowest normal x87 binade needs a pre-round tininess decision.
    // Leave that extreme boundary to the interpreter, keeping range admission
    // independent of rounding evidence on the ordinary native path.
    let native_range = zero.or(exponent.sub(2).unsigned().lt(0x7ffd));
    let sign = operands
        .left
        .bits
        .sign_exponent
        .xor(&operands.right.bits.sign_exponent)
        .and(0x8000);
    ArithmeticCandidate {
        valid: operands
            .precision_only(BinaryOperation::Multiply)
            .and(native_range),
        rounded: RoundedValue {
            value: ExtendedValue::from_precision53(Precision53::from_significand(
                normalized,
                sign.or(zero.select(0, exponent).truncate::<I16>()),
            ))
            .assume_class(Classification::zero().select(&zero, &Classification::normal())),
            inexact: nonzero.and(residual.ne(0_u64)),
            incremented,
        },
    }
}
