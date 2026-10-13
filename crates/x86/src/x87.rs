//! Numerical x87 operations produce values and evidence, without state effects.

mod add;
mod binary;
mod compare;
mod constant;
mod divide;
mod extract;
mod integer;
mod integral;
mod multiply;
mod native;
mod operand;
mod result;
mod rounding;
mod scale;
mod sqrt;
mod value;

pub(crate) use binary::BinaryOperand;
pub(crate) use compare::ComparisonKind;
pub(crate) use constant::Constant;
pub(crate) use operand::{BinaryOperands, Operand};
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
