//! Narrow sources retain their original exception evidence until consumption.

use wasm86_compiler::{Val, I1, I64};

use super::BinaryFormat;
use crate::x87::{ExtendedBits, ExtendedValue};

pub(crate) struct BinaryOperand {
    format: BinaryFormat,
    bits: Val<I64>,
    pub(crate) signaling_nan: Val<I1>,
    pub(crate) denormal: Val<I1>,
}

impl BinaryFormat {
    pub(crate) fn decode(self, bits: &Val<I64>) -> BinaryOperand {
        let nan = bits
            .and(self.sign_bit() - 1)
            .unsigned()
            .ge(self.infinity() + 1);
        BinaryOperand {
            format: self,
            bits: bits.clone(),
            signaling_nan: nan.and(bits.and(self.quiet_bit()).eq(0_u64)),
            denormal: self.denormal(bits),
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

    fn magnitude(&self) -> Val<I64> {
        self.bits.and(self.format.sign_bit() - 1)
    }

    pub(in crate::x87) fn finite(&self) -> Val<I1> {
        self.magnitude().unsigned().lt(self.format.infinity())
    }

    pub(in crate::x87) fn zero(&self) -> Val<I1> {
        self.magnitude().eq(0_u64)
    }

    pub(in crate::x87) fn infinity(&self) -> Val<I1> {
        self.magnitude().eq(self.format.infinity())
    }

    pub(in crate::x87) fn nan(&self) -> Val<I1> {
        self.magnitude().unsigned().ge(self.format.infinity() + 1)
    }
}
