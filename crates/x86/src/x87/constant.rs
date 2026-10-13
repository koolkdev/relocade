//! Constant loads round to extended precision without reporting inexact results.

use wasm86_compiler::{Val, I32};

use super::{
    rounding::RoundingInput, value::KnownClass, ExtendedBits, ExtendedValue, RoundingMode,
};

#[derive(Clone, Copy)]
pub(crate) enum Constant {
    Zero,
    One,
    Log2Ten,
    Log2E,
    Pi,
    Log10Two,
    LnTwo,
}

impl Constant {
    pub(crate) fn value(self, rounding: &RoundingMode) -> ExtendedValue {
        // Each inexact constant supplies its floor significand and whether
        // nearest rounding increments it. The discarded fraction is nonzero;
        // FLDL2T rounds down at nearest, while the other constants round up.
        let (significand, exponent, increment_nearest): (u64, u32, bool) = match self {
            Self::Zero => return ExtendedValue::from_signed_integer(&Val::<I32>::from(0)),
            Self::One => return ExtendedValue::from_signed_integer(&Val::<I32>::from(1)),
            Self::Log2Ten => (0xd49a_784b_cd1b_8afe, 0x4000, false),
            Self::Log2E => (0xb8aa_3b29_5c17_f0bb, 0x3fff, true),
            Self::Pi => (0xc90f_daa2_2168_c234, 0x4000, true),
            Self::Log10Two => (0x9a20_9a84_fbcf_f798, 0x3ffd, true),
            Self::LnTwo => (0xb172_17f7_d1cf_79ab, 0x3ffe, true),
        };
        let rounded = rounding.round(
            RoundingInput {
                integer: significand.into(),
                guard: increment_nearest.into(),
                sticky: true.into(),
            },
            &false.into(),
        );
        // PC does not apply, and the load discards rounding evidence rather
        // than publishing PE or a rounding increment in C1 (Intel Vol. 2).
        ExtendedValue::from_bits(ExtendedBits {
            significand: rounded.integer,
            sign_exponent: exponent.into(),
        })
        .assume_class(KnownClass::normal())
    }
}
