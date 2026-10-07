//! Numerical x87 operations produce values and evidence, without state effects.

mod add;
mod binary;
mod divide;
mod integer;
mod multiply;
mod native;
mod operand;
mod result;
mod rounding;
mod value;

pub(crate) use binary::{BinaryFormat, BinaryOperand};
pub(crate) use operand::BinaryOperands;
use result::BinaryArithmetic;
pub(crate) use result::{ArithmeticCandidate, ArithmeticResult, ConversionResult};
pub(crate) use rounding::RoundingMode;
pub(crate) use value::{ExtendedBits, ExtendedValue, SignOperation};

#[derive(Clone, Copy)]
pub(crate) enum BinaryOperation {
    Add,
    Subtract,
    ReverseSubtract,
    Multiply,
    Divide,
    ReverseDivide,
}
