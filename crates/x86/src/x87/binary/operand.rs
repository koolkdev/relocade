//! Narrow sources retain their original exception evidence until consumption.

use wasm86_compiler::{Val, I1, I64};

use super::{BinaryClassification, BinaryFormat};
use crate::x87::{ExtendedBits, ExtendedValue};

pub(crate) struct BinaryOperand {
    format: BinaryFormat,
    bits: Val<I64>,
    pub(crate) signaling_nan: Val<I1>,
    pub(crate) denormal: Val<I1>,
}

impl BinaryFormat {
    pub(crate) fn decode(self, bits: &Val<I64>) -> BinaryOperand {
        let class = self.classification(bits);
        BinaryOperand {
            format: self,
            bits: bits.clone(),
            signaling_nan: class.signaling_nan(),
            denormal: class.denormal(),
        }
    }
}

impl BinaryOperand {
    /// FLD quiets an SNaN after retaining its exception evidence. Keeping the
    /// narrow representation also permits exact stores back to the same format.
    pub(crate) fn loaded_value(&self) -> ExtendedValue {
        ExtendedValue::from_binary(
            self.format,
            self.bits
                .or(self.signaling_nan.select(self.format.quiet_bit(), 0_u64)),
        )
    }

    /// Arithmetic selects between original NaNs before quieting the winner.
    pub(in crate::x87) fn expanded_bits(&self) -> ExtendedBits {
        self.format.expand(&self.bits)
    }

    /// Classifies the original source before loading normalizes or quiets it.
    pub(in crate::x87) fn classification(&self) -> BinaryClassification<'_> {
        self.format.classification(&self.bits)
    }
}
