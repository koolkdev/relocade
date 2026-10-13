//! Floating-point calculations report numerical conditions without changing state.

use wasm86_compiler::{Val, I1};

/// Conditions raised by comparisons and value selection, before MXCSR masking.
pub(crate) struct SimdExceptions {
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
}
