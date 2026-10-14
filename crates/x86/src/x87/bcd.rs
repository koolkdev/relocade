//! The memory-only decimal format holds eighteen digits and a separate sign.

use wasm86_compiler::{Val, I16, I64};

use super::{value::KnownClass, ExtendedBits, ExtendedValue};

pub(crate) struct PackedBcd {
    /// Sixteen low digits, two per byte in little-endian order.
    pub(crate) low: Val<I64>,
    /// Two high digits in bits 0–7 and the sign in bit 15; bits 8–14 are ignored.
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
}
