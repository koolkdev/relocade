//! Values retain exact representations and established numerical classification.

mod classification;
mod precision53;

#[cfg(test)]
mod tests;

use wasm86_compiler::{Val, F64, I1, I16, I32, I64};

use super::BinaryFormat;
pub(super) use classification::Classification;
pub(super) use precision53::Precision53;

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

    fn normal(&self) -> Val<I1> {
        let exponent = self.exponent_field();
        exponent
            .ne(0)
            .and(exponent.ne(0x7fff))
            .and(self.significand.and(1_u64 << 63).ne(0_u64))
    }
}

#[derive(Clone)]
pub(crate) struct ExtendedValue {
    representation: Representation,
    // An exact class established by construction or a successful guard.
    // Unknown encodings retain their bits rather than acquiring a guessed class.
    class: Option<Classification>,
}

#[derive(Clone)]
enum Representation {
    Extended(ExtendedBits),
    Precision53(Precision53),
    // These are the post-load bits: SNaNs have already been quieted. Expanding
    // them reproduces the entire extended value, including the NaN payload.
    Binary {
        format: BinaryFormat,
        bits: Val<I64>,
    },
}

impl ExtendedValue {
    pub(crate) fn from_bits(bits: ExtendedBits) -> Self {
        Self {
            representation: Representation::Extended(bits),
            class: None,
        }
    }

    pub(super) fn from_binary(format: BinaryFormat, bits: Val<I64>) -> Self {
        Self {
            representation: Representation::Binary { format, bits },
            class: None,
        }
    }

    pub(super) fn from_precision53(value: Precision53) -> Self {
        Self {
            representation: Representation::Precision53(value),
            class: None,
        }
    }

    /// An exact normalized significand when the representation already proves
    /// at most 53 significant bits. The architectural sign and exponent stay separate.
    pub(super) fn precision53_significand(&self) -> Option<Val<F64>> {
        match &self.representation {
            Representation::Precision53(value) => Some(value.significand().clone()),
            Representation::Binary { .. } => {
                Some(Precision53::from_bits(&self.bits()).significand().clone())
            }
            Representation::Extended(_) => None,
        }
    }

    /// Requires a normalized or zero significand with its low 11 bits clear.
    pub(super) fn assume_precision53(mut self) -> Self {
        self.representation = Representation::Precision53(Precision53::from_bits(&self.bits()));
        self
    }

    pub(super) fn exact_bits(&self, format: BinaryFormat) -> Option<&Val<I64>> {
        match &self.representation {
            Representation::Binary {
                format: source,
                bits,
            } if *source == format => Some(bits),
            _ => None,
        }
    }

    /// An exact native value only when retained binary64 bits prove its range
    /// and precision. Precision53 alone does not bound the extended exponent.
    pub(super) fn binary64_value(&self) -> Option<Val<F64>> {
        self.exact_bits(BinaryFormat::Binary64)
            .map(Val::<F64>::from_bits)
    }

    pub(crate) fn bits(&self) -> ExtendedBits {
        match &self.representation {
            Representation::Extended(bits) => bits.clone(),
            Representation::Precision53(value) => value.bits(),
            Representation::Binary { format, bits } => format.expand(bits),
        }
    }

    pub(crate) fn or_indefinite(&self, invalid: &Val<I1>) -> Self {
        let mut value = match &self.representation {
            Representation::Precision53(value) => {
                Self::from_precision53(value.or_indefinite(invalid))
            }
            Representation::Binary { format, bits } => {
                Self::from_binary(*format, invalid.select(format.indefinite_bits(), bits))
            }
            Representation::Extended(bits) => Self::from_bits(ExtendedBits {
                significand: invalid.select(0xc000_0000_0000_0000_u64, &bits.significand),
                sign_exponent: invalid.select(0xffff_u32, &bits.sign_exponent),
            }),
        };
        value.class = self
            .class
            .as_ref()
            .map(|class| Classification::quiet_nan().select(invalid, class));
        value
    }

    /// Matching representations survive selection. Mixed representations remain
    /// exact through their extended bits.
    pub(crate) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        let representation = match (&self.representation, &otherwise.representation) {
            (Representation::Precision53(value), Representation::Precision53(other)) => {
                Representation::Precision53(value.select(condition, other))
            }
            (
                Representation::Binary { format, bits },
                Representation::Binary {
                    format: other_format,
                    bits: other_bits,
                },
            ) if format == other_format => Representation::Binary {
                format: *format,
                bits: condition.select(bits, other_bits),
            },
            _ => {
                let true_bits = self.bits();
                let false_bits = otherwise.bits();
                Representation::Extended(ExtendedBits {
                    significand: condition.select(true_bits.significand, false_bits.significand),
                    sign_exponent: condition
                        .select(true_bits.sign_exponent, false_bits.sign_exponent),
                })
            }
        };
        Self {
            representation,
            class: self
                .class
                .as_ref()
                .zip(otherwise.class.as_ref())
                .map(|(left, right)| left.select(condition, right)),
        }
    }

    pub(crate) fn tag(&self) -> Val<I16> {
        if let Some(class) = &self.class {
            // The three established classes have the same codes as their tags.
            return class.0.unsigned().extend::<I16>();
        }
        match &self.representation {
            Representation::Binary { format, bits } => format.tag(bits),
            Representation::Extended(_) | Representation::Precision53(_) => self
                .zero()
                .select(1_u32, self.normal().select(0_u32, 2_u32)),
        }
    }
}
