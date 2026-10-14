//! Exact binary32/64 expansion and directly rounded narrowing of extended values.

mod operand;

pub(crate) use operand::BinaryOperand;

use wasm86_compiler::{AtLeast, Val, I16, I32, I64};

use crate::floating::{BinaryEncoding, BinaryFormat, FiniteMagnitude, RoundingInput, RoundingMode};

use super::{
    value::{Classification, KnownClass},
    ConversionResult, ExtendedBits, ExtendedValue,
};

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

    /// Rounds the original extended value directly to the destination. PC is
    /// irrelevant to stores; no intermediate binary64 value is constructed.
    pub(crate) fn encode(self, value: &ExtendedValue, rounding: &RoundingMode) -> ConversionResult {
        if let Some(exact) = value.exact_conversion(self) {
            return exact;
        }
        match self {
            Self::Binary32 => Self::encode_rounded::<I32>(value, rounding),
            Self::Binary64 => Self::encode_rounded::<I64>(value, rounding),
        }
    }

    fn encode_rounded<T: BinaryEncoding>(
        value: &ExtendedValue,
        rounding: &RoundingMode,
    ) -> ConversionResult
    where
        I64: AtLeast<T>,
    {
        let format = T::FORMAT;
        let class = value.classification();
        let unsupported = class.unsupported();
        let special = class.special_exponent();
        let nan = class.nan();
        let signaling_nan = class.signaling_nan();
        let value = value.bits();
        let fraction_bits = format.fraction_bits();
        let shift = 63 - fraction_bits;
        let infinity = format.infinity();
        let quiet_bit = format.quiet_bit();
        let sign_bit = format.sign_bit();
        let significand = &value.significand;
        let exponent = value.exponent_field();
        let negative = value.negative();
        let sign = negative.select(sign_bit, 0_u64);
        let finite = unsupported.or(&special).eq(false);
        // E=0, including pseudo-denormals, uses the E=1 exponent. Extended
        // subnormals are far below the binary32/64 destination range.
        let rounded = FiniteMagnitude {
            significand: RoundingInput::exact(significand.clone()),
            exponent: exponent.eq(0).select(1_u32, exponent).sub(16383),
            negative: negative.clone(),
        }
        .round_binary::<T>(rounding, &finite);
        let overflow = rounded.overflow;
        let magnitude = overflow.select(
            rounding.overflow_magnitude::<T>(&negative),
            rounded.magnitude,
        );
        let nan_bits = significand
            .unsigned()
            .shr(shift)
            .and((1_u64 << fraction_bits) - 1)
            .or(quiet_bit);
        let special_bits = nan.select(nan_bits, 0_u64).or(infinity);
        let bits = unsupported.select(
            sign_bit | infinity | quiet_bit,
            special
                .select(special_bits, magnitude.unsigned().extend::<I64>())
                .or(sign),
        );
        ConversionResult {
            bits,
            invalid: unsupported.or(signaling_nan),
            overflow: overflow.clone(),
            tiny: rounded.tiny,
            inexact: finite.and(overflow.or(rounded.bounded_inexact)),
            incremented: finite.and(overflow.select(
                rounding.overflow_to_infinity(&negative),
                rounded.bounded_incremented,
            )),
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
