//! Integral rounding keeps the full extended range and ignores precision control.

use wasm86_compiler::{Val, I16, I32};

use crate::floating::BinaryFormat;

use super::{
    result::RoundedValue, rounding::RoundingInput, value::KnownClass, ArithmeticResult,
    ExtendedBits, ExtendedValue, Operand, RoundingMode,
};

impl Operand {
    pub(crate) fn round_integral(&self, rounding: &RoundingMode) -> ArithmeticResult {
        let rounded = if let Some(native) = &self.binary64_value {
            let integral = rounding.integral(native);
            RoundedValue {
                value: ExtendedValue::from_classified_binary(
                    BinaryFormat::Binary64,
                    integral.to_bits(),
                    KnownClass::zero().select(&integral.eq(0.0), &KnownClass::normal()),
                ),
                inexact: integral.ne(native),
                incremented: integral.abs().gt(native.abs()),
            }
        } else {
            self.round_extended_integral(rounding)
        };
        let mut result = ArithmeticResult::from_rounding(rounded);
        result.replace_when(
            &self.infinity,
            &RoundedValue::exact(ExtendedValue::from_bits(self.bits.clone())),
        );
        self.finish(result, false.into())
    }

    fn round_extended_integral(&self, rounding: &RoundingMode) -> RoundedValue {
        let bits = &self.bits;
        // E=0 includes true and pseudo-denormals, both using E=1's exponent.
        let exponent = bits.exponent_field().eq(0).select(1, bits.exponent_field());
        let already_integral = exponent.unsigned().ge(16383 + 63);
        let distance = already_integral.select(0, Val::<I32>::from(16383 + 63).sub(exponent));
        let rounded = rounding.round(
            RoundingInput::exact(bits.significand.clone()).shift_right(distance),
            &bits.negative(),
        );
        let shift = rounded.integer.clz().truncate::<I32>();
        let zero = rounded.integer.eq(0_u64);
        let integral_exponent = zero.select(0, Val::<I32>::from(16383 + 63).sub(&shift));
        let integral = ExtendedBits {
            significand: already_integral.select(&bits.significand, rounded.integer.shl(shift)),
            sign_exponent: already_integral.select(
                &bits.sign_exponent,
                bits.sign_exponent
                    .and(0x8000)
                    .or(integral_exponent.truncate::<I16>()),
            ),
        };
        let mut value = ExtendedValue::from_bits(integral);
        if self.precision53_significand.is_some() {
            // Integral rounding cannot add significant bits to an exact PC53 value.
            value = value.assume_precision53();
        }
        RoundedValue {
            value: value.assume_class(KnownClass::zero().select(&zero, &KnownClass::normal())),
            inexact: rounded.inexact,
            incremented: rounded.incremented,
        }
    }
}
