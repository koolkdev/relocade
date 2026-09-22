//! Exact extended-real bits, including encodings that arithmetic cannot consume.

mod binary;

pub(crate) use binary::{BinaryFormat, BinaryOperand};

use wasm86_compiler::{Val, I1, I16, I64};

#[derive(Clone)]
pub(crate) struct ExtendedValue {
    pub(crate) significand: Val<I64>,
    pub(crate) sign_exponent: Val<I16>,
}

impl ExtendedValue {
    pub(crate) fn indefinite() -> Self {
        Self {
            significand: 0xc000_0000_0000_0000_u64.into(),
            sign_exponent: 0xffff_u32.into(),
        }
    }

    pub(crate) fn or_indefinite(&self, invalid: &Val<I1>) -> Self {
        let indefinite = Self::indefinite();
        Self {
            significand: invalid.select(indefinite.significand, &self.significand),
            sign_exponent: invalid.select(indefinite.sign_exponent, &self.sign_exponent),
        }
    }

    pub(crate) fn tag(&self) -> Val<I16> {
        let exponent = self.sign_exponent.and(0x7fff);
        let zero = exponent.eq(0).and(self.significand.eq(0_u64));
        let normal = exponent
            .ne(0)
            .and(exponent.ne(0x7fff))
            .and(self.significand.unsigned().shr(63).ne(0_u64));
        zero.select(1_u32, normal.select(0_u32, 2_u32))
    }
}
