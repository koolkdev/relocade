//! Values retain exact narrow representations until an operation needs extended bits.

use wasm86_compiler::{Val, I1, I16, I32, I64};

use super::BinaryFormat;

/// Raw extended encodings include unsupported values and signaling NaNs.
#[derive(Clone)]
pub(crate) struct ExtendedBits {
    pub(crate) significand: Val<I64>,
    pub(crate) sign_exponent: Val<I16>,
}

impl ExtendedBits {
    pub(super) fn exponent_field(&self) -> Val<I32> {
        self.sign_exponent.and(0x7fff).unsigned().extend::<I32>()
    }

    pub(super) fn negative(&self) -> Val<I1> {
        self.sign_exponent.and(0x8000).ne(0)
    }

    pub(super) fn unsupported(&self) -> Val<I1> {
        self.exponent_field()
            .ne(0)
            .and(self.significand.and(1_u64 << 63).eq(0_u64))
    }

    pub(super) fn normal(&self) -> Val<I1> {
        let exponent = self.exponent_field();
        exponent
            .ne(0)
            .and(exponent.ne(0x7fff))
            .and(self.significand.and(1_u64 << 63).ne(0_u64))
    }
}

#[derive(Clone)]
pub(crate) struct ExtendedValue(Representation);

#[derive(Clone)]
enum Representation {
    Extended(ExtendedBits),
    // These are the post-load bits: SNaNs have already been quieted. Expanding
    // them reproduces the entire extended value, including the NaN payload.
    Binary {
        format: BinaryFormat,
        bits: Val<I64>,
    },
}

impl ExtendedValue {
    pub(crate) fn from_bits(bits: ExtendedBits) -> Self {
        Self(Representation::Extended(bits))
    }

    pub(super) fn from_binary(format: BinaryFormat, bits: Val<I64>) -> Self {
        Self(Representation::Binary { format, bits })
    }

    pub(super) fn exact_bits(&self, format: BinaryFormat) -> Option<&Val<I64>> {
        match &self.0 {
            Representation::Binary {
                format: source,
                bits,
            } if *source == format => Some(bits),
            _ => None,
        }
    }

    pub(crate) fn bits(&self) -> ExtendedBits {
        match &self.0 {
            Representation::Extended(bits) => bits.clone(),
            Representation::Binary { format, bits } => format.expand(bits),
        }
    }

    pub(crate) fn normal(&self) -> Val<I1> {
        self.bits().normal()
    }

    pub(crate) fn or_indefinite(&self, invalid: &Val<I1>) -> Self {
        match &self.0 {
            Representation::Binary { format, bits } => {
                Self::from_binary(*format, invalid.select(format.indefinite_bits(), bits))
            }
            Representation::Extended(bits) => Self::from_bits(ExtendedBits {
                significand: invalid.select(0xc000_0000_0000_0000_u64, &bits.significand),
                sign_exponent: invalid.select(0xffff_u32, &bits.sign_exponent),
            }),
        }
    }

    /// A conditional value keeps a narrow representation only when both arms
    /// have it. Mixed representations remain exact through their extended bits.
    pub(crate) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        if let Representation::Binary { format, bits } = &self.0 {
            if let Some(other_bits) = otherwise.exact_bits(*format) {
                return Self::from_binary(*format, condition.select(bits, other_bits));
            }
        }
        let true_bits = self.bits();
        let false_bits = otherwise.bits();
        Self::from_bits(ExtendedBits {
            significand: condition.select(true_bits.significand, false_bits.significand),
            sign_exponent: condition.select(true_bits.sign_exponent, false_bits.sign_exponent),
        })
    }

    pub(crate) fn tag(&self) -> Val<I16> {
        match &self.0 {
            Representation::Binary { format, bits } => format.tag(bits),
            Representation::Extended(bits) => {
                let exponent = bits.sign_exponent.and(0x7fff);
                let zero = exponent.eq(0).and(bits.significand.eq(0_u64));
                zero.select(1_u32, bits.normal().select(0_u32, 2_u32))
            }
        }
    }
}
