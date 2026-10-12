//! Predicates over binary32/64 encodings.

use wasm86_compiler::{Val, I1, I64};

use super::BinaryFormat;

pub(in crate::x87) struct BinaryClassification<'a> {
    format: BinaryFormat,
    bits: &'a Val<I64>,
}

impl BinaryFormat {
    pub(in crate::x87) fn classification(self, bits: &Val<I64>) -> BinaryClassification<'_> {
        BinaryClassification { format: self, bits }
    }
}

impl BinaryClassification<'_> {
    fn magnitude(&self) -> Val<I64> {
        self.bits.and(self.format.sign_bit() - 1)
    }

    pub(in crate::x87) fn finite(&self) -> Val<I1> {
        self.magnitude().unsigned().lt(self.format.infinity())
    }

    pub(in crate::x87) fn special_exponent(&self) -> Val<I1> {
        self.magnitude().unsigned().ge(self.format.infinity())
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

    pub(super) fn signaling_nan(&self) -> Val<I1> {
        self.nan()
            .and(self.bits.and(self.format.quiet_bit()).eq(0_u64))
    }

    pub(super) fn denormal(&self) -> Val<I1> {
        let magnitude = self.magnitude();
        magnitude.ne(0_u64).and(
            magnitude
                .unsigned()
                .lt(1_u64 << self.format.fraction_bits()),
        )
    }
}
