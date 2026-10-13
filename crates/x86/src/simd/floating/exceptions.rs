//! Numerical conditions are collected across lanes before MXCSR masking.

use wasm86_compiler::{Val, I1};

#[derive(Clone)]
pub(crate) struct SimdExceptions {
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) underflow: Val<I1>,
    pub(crate) precision: Val<I1>,
}

impl Default for SimdExceptions {
    fn default() -> Self {
        Self {
            invalid: false.into(),
            denormal: false.into(),
            overflow: false.into(),
            underflow: false.into(),
            precision: false.into(),
        }
    }
}

impl SimdExceptions {
    pub(crate) fn merge(&mut self, other: Self) {
        self.invalid = self.invalid.or(other.invalid);
        self.denormal = self.denormal.or(other.denormal);
        self.overflow = self.overflow.or(other.overflow);
        self.underflow = self.underflow.or(other.underflow);
        self.precision = self.precision.or(other.precision);
    }
}
