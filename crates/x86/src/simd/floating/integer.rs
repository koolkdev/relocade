//! Conversions between signed dwords and floating encodings retain rounding evidence.

use super::{FloatOperand, FloatResult, SimdExceptions};
use crate::floating::{BinaryEncoding, BinaryFormat, RoundingInput, RoundingMode};
use wasm86_compiler::{AtLeast, Val, I32, I64};

pub(crate) fn integer_to_float<T: BinaryEncoding>(
    value: Val<I32>,
    rounding: &RoundingMode,
) -> FloatResult<T>
where
    I64: AtLeast<T>,
{
    if T::FORMAT == BinaryFormat::Binary64 {
        // Every signed dword has an exact binary64 representation.
        return FloatResult {
            value: value.signed().to_f64().to_bits().truncate::<T>(),
            exceptions: SimdExceptions::default(),
        };
    }
    let negative = value.signed().lt(0);
    let magnitude = negative.select(Val::<I32>::from(0).sub(&value), value);
    let leading = magnitude.clz();
    let exponent = Val::<I32>::from(31).sub(&leading);
    let normalized = magnitude.unsigned().extend::<I64>().shl(leading.add(32));
    let rounded = rounding.round(RoundingInput::exact(normalized).shift_right(40), &negative);
    // Adding the rounded significand carries naturally into the exponent.
    let encoded = exponent
        .add(126)
        .shl(23)
        .add(rounded.integer.truncate::<I32>());
    let bits = magnitude
        .eq(0)
        .select(0, encoded)
        .or(negative.select(0x8000_0000_u32, 0));
    FloatResult {
        value: bits.unsigned().extend::<I64>().truncate::<T>(),
        exceptions: SimdExceptions {
            precision: rounded.inexact,
            ..SimdExceptions::default()
        },
    }
}

impl<T: BinaryEncoding> FloatOperand<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn to_integer(&self, rounding: &RoundingMode) -> FloatResult<I32> {
        let rounded = rounding.signed_dword(&self.value());
        FloatResult {
            value: rounded.invalid.select(0x8000_0000_u32, rounded.value),
            exceptions: SimdExceptions {
                invalid: rounded.invalid.clone(),
                precision: rounded.invalid.eq(false).and(rounded.inexact),
                // Integer conversions honor DAZ but never raise denormal operand.
                ..SimdExceptions::default()
            },
        }
    }
}
