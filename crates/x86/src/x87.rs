//! Numerical x87 operations produce values and evidence, without state effects.

mod binary;
mod integer;
mod multiply;
mod result;
mod rounding;
mod value;

pub(crate) use binary::{BinaryFormat, BinaryOperand};
pub(crate) use multiply::multiply;
pub(crate) use result::{ArithmeticResult, ConversionResult};
pub(crate) use rounding::RoundingMode;
pub(crate) use value::{ExtendedBits, ExtendedValue};
