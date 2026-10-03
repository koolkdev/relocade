//! Numerical x87 operations produce values and evidence, without state effects.

mod add;
mod binary;
mod divide;
mod integer;
mod multiply;
mod operand;
mod result;
mod rounding;
mod value;

pub(crate) use binary::{BinaryFormat, BinaryOperand};
pub(crate) use operand::BinaryOperands;
pub(crate) use result::{ArithmeticResult, BinaryArithmetic, ConversionResult};
pub(crate) use rounding::RoundingMode;
pub(crate) use value::{ExtendedBits, ExtendedValue};

#[derive(Clone, Copy)]
pub(crate) enum BinaryOperation {
    Add,
    Subtract,
    ReverseSubtract,
    Multiply,
    Divide,
    ReverseDivide,
}
