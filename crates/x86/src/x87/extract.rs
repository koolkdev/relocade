//! FXTRACT separates a normalized significand from its unbiased exponent.

use wasm86_compiler::{BlockBuilder, BuildError};

use crate::floating::BinaryFormat;

use super::{
    result::RoundedValue, value::KnownClass, ArithmeticResult, ExtendedBits, ExtendedValue, Operand,
};

pub(crate) struct Extraction {
    pub(crate) significand: ArithmeticResult,
    pub(crate) exponent: ArithmeticResult,
}

impl Operand {
    pub(crate) fn extract(&self, body: &mut BlockBuilder<'_>) -> Result<Extraction, BuildError> {
        let (significand, exponent) = self.normalized();
        let significand = match &self.precision53_significand {
            Some(native) => {
                let signed = self.bits.negative().select(native.neg(), native);
                ExtendedValue::from_classified_binary(
                    BinaryFormat::Binary64,
                    signed.to_bits(),
                    KnownClass::normal(),
                )
            }
            None => ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent: self.bits.sign_exponent.and(0x8000).or(0x3fff),
            }),
        }
        .assume_class(KnownClass::normal());
        let mut significand = significand;
        let mut exponent = ExtendedValue::from_signed_integer(&exponent);
        // A normal class established by construction or explicit refinement
        // proves the exception-free split and retains both native views.
        if body.constant_bits(self.normal_or_zero().and(self.zero.eq(false)))? == Some(1) {
            return Ok(Extraction {
                significand: ArithmeticResult::from_rounding(RoundedValue::exact(significand)),
                exponent: ArithmeticResult::from_rounding(RoundedValue::exact(exponent)),
            });
        }
        significand = ExtendedValue::from_bits(self.bits.clone())
            .select(&self.zero.or(&self.infinity), &significand);
        exponent = RoundedValue::infinity(self.zero.clone())
            .value
            .select(&self.zero.or(&self.infinity), &exponent);
        let special = self.quiet_nan().or_indefinite(&self.unsupported);
        let special_operand = self.nan.or(&self.unsupported);
        significand = special.select(&special_operand, &significand);
        exponent = special.select(&special_operand, &exponent);
        let mut exponent = ArithmeticResult::from_rounding(RoundedValue::exact(exponent));
        // Operand exceptions apply to the whole pair. Carry them once; the
        // shared register writer suppresses both destinations together.
        exponent.invalid = self.unsupported.or(&self.signaling_nan);
        exponent.denormal = exponent.invalid.or(&self.nan).eq(false).and(&self.denormal);
        exponent.zero_divide = self.zero.clone();
        Ok(Extraction {
            significand: ArithmeticResult::from_rounding(RoundedValue::exact(significand)),
            exponent,
        })
    }
}
