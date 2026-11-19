//! Exact binary32/64 expansion and directly rounded narrowing of extended values.

mod operand;
mod single;

pub(crate) use operand::BinaryOperand;

use relocade_compiler::{Val, I1, I16, I32, I64};

use crate::floating::{BinaryFormat, FiniteMagnitude, RoundingInput, RoundingMode};

use super::{
    value::{Classification, KnownClass},
    ConversionResult, ExtendedBits, ExtendedValue,
};

/// Unsigned destination encoding and the rounding evidence for a finite source.
struct StoredMagnitude {
    bits: Val<I64>,
    overflow: Val<I1>,
    tiny: Val<I1>,
    inexact: Val<I1>,
    incremented: Val<I1>,
}

impl BinaryFormat {
    /// Expands the encoding exactly, preserving an SNaN's quiet bit and payload.
    pub(super) fn expand(self, bits: &Val<I64>) -> ExtendedBits {
        self.expand_classified(bits, None)
    }

    pub(super) fn expand_classified(
        self,
        bits: &Val<I64>,
        class: Option<&KnownClass>,
    ) -> ExtendedBits {
        let fraction_bits = self.fraction_bits();
        let exponent_bits = self.exponent_bits();
        let bias = self.bias();
        let exponent_mask = (1_u64 << exponent_bits) - 1;
        let fraction = bits.and((1_u64 << fraction_bits) - 1);
        let exponent = bits.unsigned().shr(fraction_bits).and(exponent_mask);
        let sign = bits
            .unsigned()
            .shr(fraction_bits + exponent_bits)
            .truncate::<I16>()
            .shl(15);
        let (zero_exponent, special, denormal) = match class {
            Some(class) => {
                let class = Classification::known(class);
                (class.zero(), class.special_exponent(), false.into())
            }
            None => (
                exponent.eq(0_u64),
                exponent.eq(exponent_mask),
                self.classification(bits).denormal(),
            ),
        };
        let fraction = fraction.shl(63 - fraction_bits);

        // Narrow subnormals are normal extended values. This exact expansion
        // uses neither the precision control nor the rounding control fields.
        let shift = fraction.clz();
        let significand = denormal.select(
            fraction.shl(shift.truncate::<I32>()),
            fraction.or(zero_exponent.select(0_u64, 1_u64 << 63)),
        );
        let finite_exponent = denormal.select(
            Val::<I64>::from((16384 - bias) as u64).sub(shift),
            zero_exponent.select(0_u64, exponent.add((16383 - bias) as u64)),
        );
        let sign_exponent = sign.or(special
            .select(0x7fff_u64, finite_exponent)
            .truncate::<I16>());
        ExtendedBits {
            significand,
            sign_exponent,
        }
    }

    /// Stores round the original value under RC independently of the arithmetic PC.
    pub(crate) fn encode(self, value: &ExtendedValue, rounding: &RoundingMode) -> ConversionResult {
        if let Some(exact) = value.exact_conversion(self) {
            return exact;
        }
        let class = value.classification();
        let unsupported = class.unsupported();
        let special = class.special_exponent();
        let finite = unsupported.or(&special).eq(false);
        let bits = value.bits();
        let negative = value.negative();
        let magnitude = match self {
            Self::Binary32 => single::round(value, &bits, &negative, rounding),
            Self::Binary64 => bits.round_binary64(&negative, rounding),
        };
        let fraction_bits = self.fraction_bits();
        let nan_payload = bits
            .significand
            .unsigned()
            .shr(63 - fraction_bits)
            .and((1_u64 << fraction_bits) - 1)
            .or(self.quiet_bit());
        let special_bits = class.nan().select(nan_payload, 0_u64).or(self.infinity());
        let signed = special
            .select(special_bits, magnitude.bits)
            .or(negative.select(self.sign_bit(), 0_u64));
        ConversionResult {
            bits: unsupported.select(self.indefinite_bits(), signed),
            invalid: unsupported.or(class.signaling_nan()),
            overflow: finite.and(magnitude.overflow),
            tiny: finite.and(magnitude.tiny),
            inexact: finite.and(magnitude.inexact),
            incremented: finite.and(magnitude.incremented),
        }
    }

    pub(super) fn encode_exact(
        self,
        bits: &Val<I64>,
        class: Option<&KnownClass>,
    ) -> ConversionResult {
        ConversionResult {
            bits: bits.clone(),
            invalid: false.into(),
            overflow: false.into(),
            // An exact subnormal still raises underflow when UM is clear.
            // The retained native classes exclude that encoding.
            tiny: if class.is_some() {
                false.into()
            } else {
                self.classification(bits).denormal()
            },
            inexact: false.into(),
            incremented: false.into(),
        }
    }
}

impl ExtendedBits {
    fn round_binary64(&self, negative: &Val<I1>, rounding: &RoundingMode) -> StoredMagnitude {
        let exponent = self.exponent_field();
        // E=0 and E=1 share an exponent, including for pseudo-denormals.
        let rounded = FiniteMagnitude {
            significand: RoundingInput::exact(self.significand.clone()),
            exponent: exponent.eq(0).select(1_u32, exponent).sub(16383),
            negative: negative.clone(),
        }
        .round_binary::<I64>(rounding, &true.into());
        let overflow = rounded.overflow;
        StoredMagnitude {
            bits: overflow.select(
                rounding.overflow_magnitude::<I64>(negative),
                rounded.magnitude,
            ),
            inexact: overflow.or(rounded.bounded_inexact),
            incremented: overflow.select(
                rounding.overflow_to_infinity(negative),
                rounded.bounded_incremented,
            ),
            overflow,
            tiny: rounded.tiny,
        }
    }
}
