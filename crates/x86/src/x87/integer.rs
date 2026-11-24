//! Exact integer expansion and bounded rounding for integer and decimal stores.

use crate::floating::{RoundingInput, RoundingMode};
use relocade_compiler::{AtLeast, MemoryInt, Val, F64, I1, I16, I32, I64};

use super::{
    value::{KnownClass, Precision53},
    ConversionResult, ExtendedBits, ExtendedValue,
};

impl ExtendedValue {
    /// All signed integers fit exactly, regardless of PC and RC. Their source
    /// width establishes whether subsequent arithmetic can use a native significand.
    pub(crate) fn from_signed_integer<T: MemoryInt>(integer: &Val<T>) -> Self
    where
        I64: AtLeast<T>,
    {
        // Every 16/32-bit integer has an exact binary64 value. Retain it before
        // integer expansion; 64-bit inputs must keep their full precision.
        let native = (T::BYTES <= 4).then(|| integer.signed().to_f64());
        let integer = integer.signed().extend::<I64>();
        let negative = integer.signed().lt(0_u64);
        // Wrapping negation preserves the unsigned magnitude of i64::MIN.
        let magnitude = negative.select(Val::<I64>::from(0_u64).sub(&integer), &integer);
        let bits = ExtendedBits::from_integer_magnitude(&magnitude, &negative);
        let value = match native {
            Some(native) => Self::from_precision53(Precision53::from_binary64(&bits, native)),
            None => Self::from_bits(bits),
        };
        value.assume_class(KnownClass::zero().select(&integer.eq(0_u64), &KnownClass::normal()))
    }

    pub(crate) fn to_signed_integer<T: MemoryInt>(
        &self,
        rounding: &RoundingMode,
    ) -> ConversionResult {
        let sign_bit = 1_u64 << (T::BYTES * 8 - 1);
        if T::BYTES <= 4 {
            if let Some(value) = self.binary_value() {
                return native_integer_store(&value.value, rounding, sign_bit);
            }
        }
        let negative = self.bits().negative();
        let limit = negative.select(sign_bit, sign_bit - 1);
        let result = self.to_integer_magnitude(rounding, &limit);
        let invalid = result.invalid.clone();
        result.map_bits(|magnitude| {
            let integer = negative.select(Val::<I64>::from(0_u64).sub(&magnitude), magnitude);
            invalid.select(sign_bit, integer)
        })
    }

    /// Checks the rounded unsigned magnitude against the destination's limit.
    /// The destination encoder supplies its own sign and masked-invalid value.
    pub(super) fn to_integer_magnitude(
        &self,
        rounding: &RoundingMode,
        limit: &Val<I64>,
    ) -> ConversionResult {
        let value = self.bits();
        let exponent = value.exponent_field();
        let negative = value.negative();
        let unsupported = self.classification().unsupported();
        // E=0 uses the same exponent as E=1, including pseudo-denormals.
        // Integer and packed-decimal destinations cannot contain 2^64 or more.
        let exponent = exponent.eq(0).select(1_u32, exponent);
        let too_large = exponent.unsigned().ge(16383 + 64);
        let distance = too_large.select(0_u32, Val::<I32>::from(16383 + 63).sub(exponent));
        let rounded = rounding.round(
            RoundingInput::exact(value.significand).shift_right(distance),
            &negative,
        );
        // Range is checked after rounding: a fractional input can cross the limit.
        let invalid = unsupported
            .or(too_large)
            .or(limit.unsigned().lt(&rounded.integer));
        let valid = invalid.eq(false);
        ConversionResult {
            bits: rounded.integer,
            invalid,
            overflow: false.into(),
            tiny: false.into(),
            inexact: valid.and(rounded.inexact),
            incremented: valid.and(rounded.incremented),
        }
    }
}

/// Every signed 16- or 32-bit result lies in the unit-spaced binade around
/// 3 × 2^51 after biasing. The bias has an even significand, so the addition
/// rounds directly to the guest's nearest-even integer. For directed modes,
/// round first. The low bits of the biased encoding then hold the integer.
fn native_integer_store(
    value: &Val<F64>,
    rounding: &RoundingMode,
    sign_bit: u64,
) -> ConversionResult {
    const BIAS: f64 = (3_u64 << 51) as f64;
    let input = rounding.select(value.clone(), value.floor(), value.ceil(), value.trunc());
    let biased = input.add(BIAS);
    let integer = biased.to_bits().sub(BIAS.to_bits());
    // The accepted interval is contiguous in encoding order. Both endpoints
    // are positive finite encodings, so infinities and NaNs also fail this test.
    let valid = integer.add(sign_bit).unsigned().lt(2 * sign_bit);
    let rounded = biased.sub(BIAS);
    ConversionResult {
        bits: valid.select(integer, sign_bit),
        invalid: valid.eq(false),
        overflow: false.into(),
        tiny: false.into(),
        inexact: valid.and(rounded.ne(value)),
        incremented: valid.and(rounded.abs().gt(value.abs())),
    }
}

impl ExtendedBits {
    /// Exact integer expansion keeps a separate sign so decimal −0 survives.
    pub(super) fn from_integer_magnitude(magnitude: &Val<I64>, negative: &Val<I1>) -> Self {
        let shift = magnitude.clz().truncate::<I32>();
        let exponent = magnitude
            .eq(0_u64)
            .select(0_u32, Val::<I32>::from(16383 + 63).sub(&shift));
        Self {
            significand: magnitude.shl(shift),
            sign_exponent: negative
                .select(0x8000_u32, 0_u32)
                .or(exponent)
                .truncate::<I16>(),
        }
    }
}
