//! PC53/nearest arithmetic retains native significands and x87's wider exponent range.

use wasm86_compiler::{Val, F64, I1, I16, I32, I64};

use super::{
    operand::{AddendSigns, BinaryOperands, Operand},
    result::RoundedValue,
    value::{Classification, Precision53},
    ArithmeticCandidate, BinaryOperation, ExtendedValue,
};

fn integer_significand(value: &Val<F64>) -> Val<I64> {
    value.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52)
}

/// Requires PC53/nearest. Wider operands keep the general calculation.
pub(super) fn calculate(
    operands: &BinaryOperands,
    operation: BinaryOperation,
) -> Option<ArithmeticCandidate> {
    let left = operands.left.precision53_significand.as_ref()?;
    let right = operands.right.precision53_significand.as_ref()?;
    let result = match operation {
        BinaryOperation::Multiply => multiply(operands, left, right),
        BinaryOperation::Divide => divide(&operands.left, &operands.right, left, right),
        BinaryOperation::ReverseDivide => divide(&operands.right, &operands.left, right, left),
        BinaryOperation::Add | BinaryOperation::Subtract | BinaryOperation::ReverseSubtract => {
            add(operands, operation, left, right)
        }
    };
    Some(result.candidate(operands, operation))
}

/// A normalized native result with architectural sign and rounding evidence.
struct NativeResult {
    significand: Val<F64>,
    biased_exponent: Val<I32>,
    zero: Val<I1>,
    sign: Val<I16>,
    inexact: Val<I1>,
    incremented: Val<I1>,
}

fn multiply(operands: &BinaryOperands, left: &Val<F64>, right: &Val<F64>) -> NativeResult {
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
    NativeResult {
        significand: carry.select(product.mul(0.5), &product),
        biased_exponent,
        zero: operands.left.zero.or(&operands.right.zero),
        sign: operands
            .left
            .bits
            .sign_exponent
            .xor(&operands.right.bits.sign_exponent)
            .and(0x8000),
        inexact: residual.ne(0_u64),
        incremented: residual.signed().lt(0),
    }
}

fn divide(
    dividend: &Operand,
    divisor: &Operand,
    numerator: &Val<F64>,
    denominator: &Val<F64>,
) -> NativeResult {
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
    NativeResult {
        significand: below_one.select(quotient.mul(2.0), &quotient),
        biased_exponent,
        zero: dividend.zero.clone(),
        sign: dividend
            .bits
            .sign_exponent
            .xor(&divisor.bits.sign_exponent)
            .and(0x8000),
        inexact: residual.ne(0_u64),
        incremented: residual.signed().lt(0),
    }
}

fn add(
    operands: &BinaryOperands,
    operation: BinaryOperation,
    left: &Val<F64>,
    right: &Val<F64>,
) -> NativeResult {
    let AddendSigns {
        left_negative,
        right_negative,
    } = operands.addend_signs(operation);
    let left = left_negative.select(left.neg(), left);
    let right = right_negative.select(right.neg(), right);
    let left_exponent = operands.left.bits.exponent_field();
    let right_exponent = operands.right.bits.exponent_field();
    let left_first = left_exponent.unsigned().ge(&right_exponent);
    let exponent = left_first.select(&left_exponent, &right_exponent);
    let distance = exponent.sub(left_first.select(right_exponent, left_exponent));
    // Beyond 55 bits, even the largest smaller coefficient cannot cross the
    // midpoint below 1. Clamping preserves the result and rounding evidence.
    let distance = distance.unsigned().ge(56).select(55, distance);
    let scale = Val::<F64>::from_bits(
        Val::<I32>::from(1023)
            .sub(distance)
            .unsigned()
            .extend::<I64>()
            .shl(52),
    );
    let leading = left_first.select(&left, &right);
    let aligned = left_first.select(right, left).mul(scale);
    let sum = leading.add(&aligned);
    // FastTwoSum recovers the exact error of the aligned sum. Exponent ordering is
    // sufficient, including equal exponents with either significand order.
    let error = aligned.sub(sum.sub(leading));
    let bits = sum.to_bits();
    let zero = sum.eq(0.0);
    let sign = bits.unsigned().shr(48).truncate::<I16>().and(0x8000);
    let biased_exponent = exponent
        .add(bits.unsigned().shr(52).truncate::<I32>().and(0x7ff))
        .sub(1023);
    let significand = Val::<F64>::from_bits(
        bits.and((1_u64 << 52) - 1)
            .or(zero.select(0_u64, 1023_u64 << 52)),
    );
    NativeResult {
        significand,
        biased_exponent,
        zero,
        inexact: error.ne(0.0),
        incremented: error.ne(0.0).and(error.lt(0.0).xor(sign.ne(0))),
        sign,
    }
}

impl NativeResult {
    fn candidate(
        self,
        operands: &BinaryOperands,
        operation: BinaryOperation,
    ) -> ArithmeticCandidate {
        let Self {
            significand,
            biased_exponent,
            zero,
            sign,
            inexact,
            incremented,
        } = self;
        let nonzero = zero.eq(false);
        // The lowest normal x87 binade needs a pre-round tininess decision.
        // Leave that extreme boundary to the interpreter, keeping range admission
        // independent of rounding evidence on the ordinary native path.
        let native_range = zero.or(biased_exponent.sub(2).unsigned().lt(0x7ffd));
        ArithmeticCandidate {
            valid: operands.precision_only(operation).and(native_range),
            rounded: RoundedValue {
                value: ExtendedValue::from_precision53(Precision53::from_significand(
                    significand,
                    sign.or(zero.select(0, biased_exponent).truncate::<I16>()),
                ))
                .assume_class(Classification::zero().select(&zero, &Classification::normal())),
                inexact: nonzero.and(inexact),
                incremented: nonzero.and(incremented),
            },
        }
    }
}
