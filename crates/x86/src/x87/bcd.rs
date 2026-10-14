//! The memory-only decimal format holds eighteen digits and a separate sign.

use crate::floating::RoundingMode;
use wasm86_compiler::{Val, I1, I16, I32, I64};

use super::{value::KnownClass, ConversionResult, ExtendedBits, ExtendedValue};

pub(crate) struct PackedBcd {
    /// Sixteen low digits, two per byte in little-endian order.
    pub(crate) low: Val<I64>,
    /// Two high digits in bits 0–7 and the sign in bit 15; bits 8–14 are reserved.
    pub(crate) high: Val<I16>,
}

impl PackedBcd {
    pub(crate) fn decode(&self) -> ExtendedValue {
        let mut magnitude = Val::<I64>::from(0_u64);
        for index in (0..18).rev() {
            let digit = if index < 16 {
                self.low.unsigned().shr(index * 4).and(0xf_u64)
            } else {
                self.high
                    .unsigned()
                    .shr((index - 16) * 4)
                    .and(0xf)
                    .unsigned()
                    .extend::<I64>()
            };
            // FBLD does not check malformed A–F digits; their result is undefined.
            magnitude = magnitude.mul(10_u64).add(digit);
        }
        let negative = self.high.and(0x8000).ne(0);
        ExtendedValue::from_bits(ExtendedBits::from_integer_magnitude(&magnitude, &negative))
            .assume_class(KnownClass::zero().select(&magnitude.eq(0_u64), &KnownClass::normal()))
    }

    fn encode(mut magnitude: Val<I64>, negative: &Val<I1>) -> Self {
        let mut low = Val::<I64>::from(0_u64);
        let mut high = negative.select::<I16>(0x8000, 0);
        for byte in 0..9 {
            let pair = magnitude.unsigned().rem(100_u64).truncate::<I32>();
            let digits = pair.unsigned().div(10).shl(4).or(pair.unsigned().rem(10));
            if byte < 8 {
                low = low.or(digits.unsigned().extend::<I64>().shl(byte * 8));
            } else {
                high = high.or(digits.truncate::<I16>());
            }
            magnitude = magnitude.unsigned().div(100_u64);
        }
        Self { low, high }
    }
}

impl ExtendedValue {
    pub(crate) fn to_packed_bcd(&self, rounding: &RoundingMode) -> ConversionResult<PackedBcd> {
        let negative = self.bits().negative();
        let result = self.to_integer_magnitude(rounding, &999_999_999_999_999_999_u64.into());
        let invalid = result.invalid.clone();
        result.map_bits(|magnitude| {
            let bcd = PackedBcd::encode(magnitude, &negative);
            // The architectural decimal indefinite is FFFF C000 0000 0000 0000.
            PackedBcd {
                low: invalid.select(0xc000_0000_0000_0000_u64, bcd.low),
                high: invalid.select(0xffff, bcd.high),
            }
        })
    }
}
