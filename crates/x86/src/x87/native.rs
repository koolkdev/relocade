//! PC53/nearest arithmetic retains native significands and x87's wider exponent range.

use wasm86_compiler::{Val, F64, I1, I16, I32, I64};

use super::{
    operand::{BinaryOperands, Operand},
    result::RoundedValue,
    value::{Classification, Precision53},
    ArithmeticCandidate, BinaryOperation, ExtendedValue,
};

fn integer_significand(value: &Val<F64>) -> Val<I64> {
    value.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52)
}

/// Requires PC53/nearest. Wider operands keep their integer calculation.
pub(super) fn calculate(
    operands: &BinaryOperands,
    operation: BinaryOperation,
) -> Option<ArithmeticCandidate> {
    let left = operands.left.precision53_significand.as_ref()?;
    let right = operands.right.precision53_significand.as_ref()?;
    let magnitude = match operation {
        BinaryOperation::Multiply => multiply(operands, left, right),
        BinaryOperation::Divide => divide(&operands.left, &operands.right, left, right),
        BinaryOperation::ReverseDivide => divide(&operands.right, &operands.left, right, left),
        _ => return None,
    };
    Some(magnitude.candidate(operands, operation))
}

/// A normalized native result, with an exact signed residual for admitted nonzero inputs.
struct RoundedMagnitude {
    significand: Val<F64>,
    biased_exponent: Val<I32>,
    zero: Val<I1>,
    residual: Val<I64>,
}

fn multiply(operands: &BinaryOperands, left: &Val<F64>, right: &Val<F64>) -> RoundedMagnitude {
    // Nonzero coefficients are in [1, 2); their product cannot overflow or
    // underflow binary64. Scaling by one half is exact at this magnitude.
    let product = left.mul(right);
    let carry = product.ge(2.0);
    let biased_exponent = operands
        .left
        .bits
        .exponent_field()
        .add(operands.right.bits.exponent_field())
        .sub(16383)
        .add(carry.unsigned().extend::<I32>());
    // A*B - C*2^shift fits in signed 54 bits, so its low word is exact.
    let residual = integer_significand(left)
        .mul(integer_significand(right))
        .sub(integer_significand(&product).shl(carry.select(53, 52)));
    RoundedMagnitude {
        significand: carry.select(product.mul(0.5), &product),
        biased_exponent,
        zero: operands.left.zero.or(&operands.right.zero),
        residual,
    }
}

fn divide(
    dividend: &Operand,
    divisor: &Operand,
    numerator: &Val<F64>,
    denominator: &Val<F64>,
) -> RoundedMagnitude {
    // Nonzero quotients lie in [0.5, 2); scaling by two is exact. With 53-bit
    // inputs a quotient below one cannot round across that boundary.
    let quotient = numerator.div(denominator);
    let below_one = quotient.lt(1.0);
    let biased_exponent = dividend
        .bits
        .exponent_field()
        .sub(divisor.bits.exponent_field())
        .add(16383)
        .sub(below_one.unsigned().extend::<I32>());
    // For 53-bit A, B and rounded C, R = A*2^(52+below_one) - B*C has
    // |R| <= B/2 < 2^52. Its low word preserves both exactness and direction.
    let residual = integer_significand(numerator)
        .shl(below_one.select(53, 52))
        .sub(integer_significand(denominator).mul(integer_significand(&quotient)));
    RoundedMagnitude {
        significand: below_one.select(quotient.mul(2.0), &quotient),
        biased_exponent,
        zero: dividend.zero.clone(),
        residual,
    }
}

impl RoundedMagnitude {
    fn candidate(
        self,
        operands: &BinaryOperands,
        operation: BinaryOperation,
    ) -> ArithmeticCandidate {
        let Self {
            significand,
            biased_exponent,
            zero,
            residual,
        } = self;
        let nonzero = zero.eq(false);
        // The lowest normal x87 binade needs a pre-round tininess decision.
        // Leave that extreme boundary to the interpreter, keeping range admission
        // independent of rounding evidence on the ordinary native path.
        let native_range = zero.or(biased_exponent.sub(2).unsigned().lt(0x7ffd));
        let sign = operands
            .left
            .bits
            .sign_exponent
            .xor(&operands.right.bits.sign_exponent)
            .and(0x8000);
        ArithmeticCandidate {
            valid: operands.precision_only(operation).and(native_range),
            rounded: RoundedValue {
                value: ExtendedValue::from_precision53(Precision53::from_significand(
                    significand,
                    sign.or(zero.select(0, biased_exponent).truncate::<I16>()),
                ))
                .assume_class(Classification::zero().select(&zero, &Classification::normal())),
                inexact: nonzero.and(residual.ne(0_u64)),
                incremented: nonzero.and(residual.signed().lt(0)),
            },
        }
    }
}
