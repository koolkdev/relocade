//! FXTRACT separates a normalized significand from its unbiased exponent.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1};

use crate::floating::BinaryFormat;

use super::{result::RoundedValue, value::KnownClass, ExtendedBits, ExtendedValue, Operand};

pub(crate) struct Extraction {
    pub(crate) significand: ExtendedValue,
    pub(crate) exponent: ExtendedValue,
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) zero_divide: Val<I1>,
}

impl Operand {
    pub(crate) fn extract(&self, body: &mut BlockBuilder<'_>) -> Result<Extraction, BuildError> {
        let (significand, exponent) = self.normalized();
        let significand = match &self.precision53_significand {
            Some(native) => {
                let signed = self.bits.negative().select(native.neg(), native);
                ExtendedValue::from_binary(BinaryFormat::Binary64, signed.to_bits())
            }
            None => ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent: self.bits.sign_exponent.and(0x8000).or(0x3fff),
            }),
        }
        .assume_class(KnownClass::normal());
        let mut result = Extraction {
            significand,
            exponent: ExtendedValue::from_signed_integer(&exponent),
            invalid: false.into(),
            denormal: false.into(),
            zero_divide: false.into(),
        };
        // A normal class established by construction or explicit refinement
        // proves the exception-free split and retains both native views.
        if body.constant_bits(self.normal_or_zero().and(self.zero.eq(false)))? == Some(1) {
            return Ok(result);
        }
        result.significand = ExtendedValue::from_bits(self.bits.clone())
            .select(&self.zero.or(&self.infinity), &result.significand);
        result.exponent = RoundedValue::infinity(self.zero.clone())
            .value
            .select(&self.zero.or(&self.infinity), &result.exponent);
        let special = self.quiet_nan().or_indefinite(&self.unsupported);
        let special_operand = self.nan.or(&self.unsupported);
        result.significand = special.select(&special_operand, &result.significand);
        result.exponent = special.select(&special_operand, &result.exponent);
        result.invalid = self.unsupported.or(&self.signaling_nan);
        result.denormal = result.invalid.or(&self.nan).eq(false).and(&self.denormal);
        result.zero_divide = self.zero.clone();
        Ok(result)
    }
}
