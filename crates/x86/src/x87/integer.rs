//! Exact integer expansion and rounded signed-integer narrowing.

use wasm86_compiler::{MemoryInt, Val, I16, I32, I64};

use super::{rounding::RoundingInput, ConversionResult, ExtendedBits, ExtendedValue, RoundingMode};

impl ExtendedValue {
    /// All signed 64-bit integers fit exactly, regardless of PC and RC.
    pub(crate) fn from_signed_integer(integer: &Val<I64>) -> Self {
        let negative = integer.signed().lt(0_u64);
        // Wrapping negation preserves the unsigned magnitude of i64::MIN.
        let magnitude = negative.select(Val::<I64>::from(0_u64).sub(integer), integer);
        let shift = magnitude.clz().truncate::<I32>();
        let exponent = magnitude
            .eq(0_u64)
            .select(0_u32, Val::<I32>::from(16383 + 63).sub(&shift));
        Self::from_bits(ExtendedBits {
            significand: magnitude.shl(shift),
            sign_exponent: negative
                .select(0x8000_u32, 0_u32)
                .or(exponent)
                .truncate::<I16>(),
        })
    }

    pub(crate) fn to_signed_integer<T: MemoryInt>(
        &self,
        rounding: &RoundingMode,
    ) -> ConversionResult {
        let value = self.bits();
        let exponent = value.exponent_field();
        let negative = value.negative();
        let unsupported = self.unsupported();
        // E=0 uses the same exponent as E=1, including pseudo-denormals.
        // No signed destination can contain a magnitude of 2^64 or more.
        let exponent = exponent.eq(0).select(1_u32, exponent);
        let too_large = exponent.unsigned().ge(16383 + 64);
        let distance = too_large.select(0_u32, Val::<I32>::from(16383 + 63).sub(exponent));
        let rounded = rounding.round(
            RoundingInput::exact(value.significand).shift_right(distance),
            &negative,
        );
        let sign_bit = 1_u64 << (T::BYTES * 8 - 1);
        let limit = negative.select(sign_bit, sign_bit - 1);
        // Test the rounded magnitude before applying the sign. The negative
        // limit has one extra value, and a rounded result can cross either limit.
        let invalid = unsupported
            .or(too_large)
            .or(limit.unsigned().lt(&rounded.integer));
        let integer = negative.select(
            Val::<I64>::from(0_u64).sub(&rounded.integer),
            rounded.integer,
        );
        let valid = invalid.eq(false);
        ConversionResult {
            bits: invalid.select(sign_bit, integer),
            invalid,
            overflow: false.into(),
            tiny: false.into(),
            inexact: valid.and(rounded.inexact),
            incremented: valid.and(rounded.incremented),
        }
    }
}
