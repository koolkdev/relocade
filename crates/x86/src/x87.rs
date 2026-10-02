//! Numerical x87 operations produce values and evidence, without state effects.

mod arithmetic;
mod binary;
mod integer;
mod multiply;
mod rounding;
mod value;

pub(crate) use arithmetic::ArithmeticResult;
pub(crate) use binary::{BinaryFormat, BinaryOperand};
pub(crate) use multiply::multiply;
pub(crate) use rounding::RoundingMode;
pub(crate) use value::{ExtendedBits, ExtendedValue};

use wasm86_compiler::{Val, I1, I64};

/// Result bits and numerical conditions from conversion to the destination format.
/// The x87 state applies exception masks to these conditions.
///
/// Invalid conversions do not report inexactness or a rounding increment.
pub(crate) struct ConversionResult {
    pub(crate) bits: Val<I64>,
    pub(crate) invalid: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(crate) inexact: Val<I1>,
    /// Rounding increased the magnitude.
    pub(crate) incremented: Val<I1>,
}
