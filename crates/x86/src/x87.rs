//! Numerical x87 operations produce values and evidence, without state effects.

mod add;
mod arctangent;
mod bcd;
mod binary;
mod compare;
mod constant;
mod divide;
mod exponential;
mod extract;
mod integer;
mod integral;
mod logarithm;
mod multiply;
mod native;
mod operand;
mod quotient;
mod remainder;
mod result;
mod rounding;
mod scale;
mod sqrt;
mod transcendental;
mod trigonometric;
mod value;

pub(crate) use bcd::PackedBcd;
pub(crate) use binary::BinaryOperand;
pub(crate) use compare::ComparisonKind;
pub(crate) use constant::Constant;
pub(crate) use logarithm::LogarithmArgument;
pub(crate) use operand::{BinaryOperands, Operand};
pub(crate) use remainder::QuotientRounding;
use result::BinaryArithmetic;
pub(crate) use result::{ArithmeticCandidate, ArithmeticResult, ConversionResult};
pub(crate) use trigonometric::{TrigonometricArgument, TrigonometricFunction};
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
