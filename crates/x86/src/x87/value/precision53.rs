//! A binary64 significand retains native SSA values without narrowing x87's exponent.

use wasm86_compiler::{Val, F64, I1, I16};

use super::{ExtendedBits, SignOperation};

/// The positive significand is in [1, 2), or +0. The architectural sign and
/// exponent are separate, including for special encodings such as indefinite.
#[derive(Clone)]
pub(in crate::x87) struct Precision53 {
    significand: Val<F64>,
    // Both exact views are pure expressions. Keep the original one so an
    // integer consumer does not pay for an unused native conversion or reverse it.
    bits: ExtendedBits,
    // Present only when construction also proves binary64's exponent range.
    binary64_value: Option<Val<F64>>,
}

impl Precision53 {
    /// Requires a normalized or zero significand with its low 11 bits clear.
    pub(in crate::x87) fn from_bits(bits: &ExtendedBits) -> Self {
        let fraction = bits.significand.unsigned().shr(11).and((1_u64 << 52) - 1);
        let exponent = bits.significand.eq(0_u64).select(0_u64, 1023_u64 << 52);
        Self {
            significand: Val::<F64>::from_bits(fraction.or(exponent)),
            bits: bits.clone(),
            binary64_value: None,
        }
    }

    /// Requires the normalized or zero encoding accepted by `from_bits`,
    /// together with an exact numerical binary64 view of those bits.
    /// The encoded view retains signed zero and the complete NaN payload.
    pub(in crate::x87) fn from_binary64(bits: &ExtendedBits, value: Val<F64>) -> Self {
        Self {
            binary64_value: Some(value),
            ..Self::from_bits(bits)
        }
    }

    /// Requires a positive significand in [1, 2), or +0. The encoded view
    /// reproduces that coefficient exactly with the separate sign and exponent.
    pub(in crate::x87) fn from_significand(significand: Val<F64>, sign_exponent: Val<I16>) -> Self {
        let bits = significand.to_bits();
        let encoded = bits
            .and((1_u64 << 52) - 1)
            .or(bits.eq(0_u64).select(0_u64, 1_u64 << 52))
            .shl(11);
        Self {
            significand,
            bits: ExtendedBits {
                significand: encoded,
                sign_exponent,
            },
            binary64_value: None,
        }
    }

    pub(super) fn bits(&self) -> &ExtendedBits {
        &self.bits
    }

    pub(in crate::x87) fn significand(&self) -> &Val<F64> {
        &self.significand
    }

    pub(super) fn binary64_value(&self) -> Option<&Val<F64>> {
        self.binary64_value.as_ref()
    }

    pub(super) fn change_sign(&self, operation: SignOperation) -> Self {
        Self {
            significand: self.significand.clone(),
            bits: ExtendedBits {
                significand: self.bits.significand.clone(),
                sign_exponent: operation.apply(&self.bits.sign_exponent, 0x8000),
            },
            binary64_value: self.binary64_value.as_ref().map(|value| match operation {
                SignOperation::Negate => value.neg(),
                SignOperation::Absolute => value.abs(),
            }),
        }
    }

    pub(super) fn or_indefinite(&self, invalid: &Val<I1>) -> Self {
        Self {
            // The coefficient 1.5 supplies the canonical quiet payload. The
            // architectural exponent, rather than the F64 carrier, encodes NaN.
            significand: invalid.select(1.5, &self.significand),
            bits: ExtendedBits {
                significand: invalid.select(0xc000_0000_0000_0000_u64, &self.bits.significand),
                sign_exponent: invalid.select(0xffff, &self.bits.sign_exponent),
            },
            binary64_value: self.binary64_value.as_ref().map(|value| {
                invalid.select(Val::<F64>::from_bits(0xfff8_0000_0000_0000_u64), value)
            }),
        }
    }

    pub(super) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        Self {
            significand: condition.select(&self.significand, &otherwise.significand),
            bits: ExtendedBits {
                significand: condition.select(&self.bits.significand, &otherwise.bits.significand),
                sign_exponent: condition
                    .select(&self.bits.sign_exponent, &otherwise.bits.sign_exponent),
            },
            binary64_value: self
                .binary64_value
                .as_ref()
                .zip(otherwise.binary64_value.as_ref())
                .map(|(value, other)| condition.select(value, other)),
        }
    }
}
