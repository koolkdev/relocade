//! Exact PC53 inputs use one native root and an exact low-word residual.

use wasm86_compiler::{Val, F64, I16, I64};

use crate::floating::BinaryFormat;

use super::super::{
    result::RoundedValue,
    value::{KnownClass, Precision53},
    ExtendedValue, Operand,
};

fn integer_significand(value: &Val<F64>) -> Val<I64> {
    value.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52)
}

/// Requires a nonnegative normal operand or signed zero, and PC53/nearest.
pub(super) fn calculate(operand: &Operand) -> Option<RoundedValue> {
    let coefficient = operand.precision53_significand.as_ref()?;
    let exponent = operand.bits.exponent_field().sub(16383);
    let odd = exponent.and(1).ne(0);
    let class = KnownClass::zero().select(&operand.zero, &KnownClass::normal());
    let (root, value) = match &operand.binary64_value {
        Some(native) => {
            // A binary64 operand's root fits its range. Retain the whole
            // result so a following exact store does not re-encode it.
            let root = native.sqrt();
            let value = ExtendedValue::from_classified_binary(
                BinaryFormat::Binary64,
                root.to_bits(),
                class.clone(),
            );
            (root, value)
        }
        None => {
            // The coefficient has its own bounded range even when the
            // architectural value lies outside binary64's exponent range.
            let root = odd.select(coefficient.mul(2.0), coefficient).sqrt();
            let biased = exponent.signed().shr(1).add(16383);
            let sign_exponent = operand.zero.select(
                operand.bits.sign_exponent.and(0x8000),
                biased.truncate::<I16>(),
            );
            let value = ExtendedValue::from_precision53(Precision53::from_significand(
                root.clone(),
                sign_exponent,
            ));
            (root, value)
        }
    };
    // A*2^(52+odd) - C^2 has magnitude below 2^53: nearest rounding
    // changes the exact root by at most half a unit and C < 2^53.
    // Its low word therefore retains both the exactness and rounding direction.
    let residual = integer_significand(coefficient)
        .shl(odd.select(53, 52))
        .sub(integer_significand(&root).mul(integer_significand(&root)));
    let nonzero = operand.zero.eq(false);
    Some(RoundedValue {
        value: value.assume_class(class),
        inexact: nonzero.and(residual.ne(0_u64)),
        incremented: nonzero.and(residual.signed().lt(0)),
    })
}
