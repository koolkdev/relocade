//! FSCALE changes the extended exponent at full precision, leaving ST(1) intact.

use wasm86_compiler::{Val, I16, I32};

use super::{
    result::RoundedValue,
    rounding::{FiniteMagnitude, RoundingInput},
    value::{KnownClass, Precision53},
    ArithmeticResult, BinaryOperands, ExtendedBits, ExtendedValue, Operand, RoundingMode,
};

impl Operand {
    /// Truncates the scale toward zero. Beyond ±2^17 every finite nonzero ST(0)
    /// overflows or underflows even after the unmasked exponent adjustment,
    /// so a bounded exponent retains all architecturally distinct responses.
    fn scale_exponent(&self) -> Val<I32> {
        let exponent = self.bits.exponent_field();
        let magnitude = exponent.unsigned().lt(16383).select(
            0,
            exponent.unsigned().ge(16383 + 17).select(
                1 << 17,
                self.bits
                    .significand
                    .unsigned()
                    .shr(Val::<I32>::from(16383 + 63).sub(exponent))
                    .truncate::<I32>(),
            ),
        );
        self.bits
            .negative()
            .select(Val::<I32>::from(0).sub(&magnitude), magnitude)
    }
}

impl BinaryOperands {
    pub(crate) fn scale(&self, rounding: &RoundingMode) -> ArithmeticResult {
        let (significand, exponent) = self.left.normalized();
        let exponent = exponent.add(self.right.scale_exponent());
        let negative = self.left.bits.negative();
        let zero = RoundedValue::zero(negative.clone());
        let infinity = RoundedValue::infinity(negative.clone());
        let magnitude = FiniteMagnitude {
            significand: RoundingInput::exact(significand.clone()),
            exponent: exponent.clone(),
            negative: negative.clone(),
        };
        // FSCALE is independent of PC, including gradual underflow rounding.
        let mut result = magnitude.round(3.into(), rounding);
        let sign_exponent = negative
            .select(0x8000, 0)
            .or(self.left.zero.select(0, exponent.add(16383)))
            .truncate::<I16>();
        let value = match &self.left.precision53_significand {
            Some(native) => ExtendedValue::from_precision53(Precision53::from_significand(
                native.clone(),
                sign_exponent,
            )),
            None => ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent,
            }),
        };
        result.in_range.valid = self.left.zero.or(&result.in_range.valid);
        result.in_range.rounded = RoundedValue::exact(
            value.assume_class(KnownClass::zero().select(&self.left.zero, &KnownClass::normal())),
        );

        // Unlike multiplication/division, the exponent may remain outside the
        // normal range after biasing by 24576. Intel specifies infinity or zero
        // in these massive cases regardless of RC, with an inexact response.
        let massive_overflow = exponent.signed().ge(40960);
        let massive_underflow = exponent.signed().lt(-40958);
        let massive = RoundedValue {
            value: infinity.value.select(&massive_overflow, &zero.value),
            inexact: true.into(),
            incremented: massive_overflow.clone(),
        };
        result.adjusted = massive.select(&massive_overflow.or(massive_underflow), &result.adjusted);

        let negative_infinity = self.right.infinity.and(self.right.bits.negative());
        let positive_infinity = self
            .right
            .infinity
            .and(self.right.bits.negative().eq(false));
        result.replace_when(&self.left.zero.or(&negative_infinity), &zero);
        result.replace_when(&self.left.infinity.or(&positive_infinity), &infinity);
        let invalid = self
            .left
            .zero
            .and(positive_infinity)
            .or(self.left.infinity.and(negative_infinity));
        self.finish(result, invalid)
    }
}
