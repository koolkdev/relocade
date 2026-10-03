//! Binary operands share normalization, NaN selection and exception priority.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64, I8};

use super::{
    result::RoundedValue, ArithmeticResult, BinaryArithmetic, BinaryOperand, BinaryOperation,
    ExtendedBits, ExtendedValue, RoundingMode,
};

pub(super) struct Operand {
    pub(super) bits: ExtendedBits,
    normal_or_zero: Val<I1>,
    pub(super) zero: Val<I1>,
    pub(super) infinity: Val<I1>,
    nan: Val<I1>,
    signaling_nan: Val<I1>,
    denormal: Val<I1>,
    unsupported: Val<I1>,
}

impl Operand {
    fn new(value: &ExtendedValue) -> Self {
        Self {
            bits: value.bits(),
            normal_or_zero: value.normal().or(value.zero()),
            zero: value.zero(),
            infinity: value.infinity(),
            nan: value.nan(),
            signaling_nan: value.signaling_nan(),
            denormal: value.denormal(),
            unsupported: value.unsupported(),
        }
    }

    fn from_binary(source: &BinaryOperand) -> Self {
        Self {
            bits: source.expanded_bits(),
            // Every finite narrow value expands to normal binary80 or zero.
            // A narrow denormal still carries #D evidence for this operation.
            normal_or_zero: source.finite(),
            zero: source.zero(),
            infinity: source.infinity(),
            nan: source.nan(),
            signaling_nan: source.signaling_nan.clone(),
            denormal: source.denormal.clone(),
            unsupported: false.into(),
        }
    }

    pub(super) fn normalized(&self) -> (Val<I64>, Val<I32>) {
        // Normal and zero inputs retain their significands. Other classes use
        // denormal normalization; special-operand responses discard that result.
        // The admission predicate also lets an earlier guard remove this work.
        let ordinary = &self.normal_or_zero;
        let shift = ordinary.select(0, self.bits.significand.clz().truncate::<I32>());
        let exponent = ordinary.select(self.bits.exponent_field(), 1);
        (
            self.bits.significand.shl(&shift),
            exponent.sub(16383).sub(shift),
        )
    }
}

pub(crate) struct BinaryOperands {
    pub(super) left: Operand,
    pub(super) right: Operand,
    precision_only: Val<I1>,
}

impl BinaryOperands {
    pub(crate) fn new(left: &ExtendedValue, right: &ExtendedValue) -> Self {
        let left = Operand::new(left);
        let right = Operand::new(right);
        let precision_only = left.normal_or_zero.and(&right.normal_or_zero);
        Self {
            left,
            right,
            precision_only,
        }
    }

    pub(crate) fn from_binary(left: &ExtendedValue, right: &BinaryOperand) -> Self {
        let left = Operand::new(left);
        let right = Operand::from_binary(right);
        let precision_only = left
            .normal_or_zero
            .and(&right.normal_or_zero)
            .and(right.denormal.eq(false));
        Self {
            left,
            right,
            precision_only,
        }
    }

    /// Normal or zero values with no operand exception for this operation.
    /// Division also excludes a zero divisor. Narrow denormals expand to
    /// normal binary80 but still require a #D response.
    pub(crate) fn precision_only(&self, operation: BinaryOperation) -> Val<I1> {
        match operation {
            BinaryOperation::Divide => self.precision_only.and(self.right.zero.eq(false)),
            BinaryOperation::ReverseDivide => self.precision_only.and(self.left.zero.eq(false)),
            _ => self.precision_only.clone(),
        }
    }

    pub(crate) fn calculate(
        &self,
        body: &mut BlockBuilder<'_>,
        operation: BinaryOperation,
        precision: Val<I8>,
        rounding: &RoundingMode,
    ) -> Result<BinaryArithmetic, BuildError> {
        match operation {
            BinaryOperation::Multiply => Ok(super::multiply::multiply(self, precision, rounding)),
            BinaryOperation::Divide | BinaryOperation::ReverseDivide => {
                Ok(super::divide::divide(self, operation, precision, rounding))
            }
            BinaryOperation::Add | BinaryOperation::Subtract | BinaryOperation::ReverseSubtract => {
                super::add::add(body, self, operation, precision, rounding)
            }
        }
    }

    /// Resolves NaNs and operand exceptions after the operation supplies its
    /// zero/infinity responses and invalid combination. NaN selection retains
    /// the original destination/source order even for reversed arithmetic.
    pub(super) fn finish(
        &self,
        mut result: ArithmeticResult,
        invalid_operation: Val<I1>,
    ) -> ArithmeticResult {
        let Self { left, right, .. } = self;
        let indefinite = left
            .unsupported
            .or(&right.unsupported)
            .or(invalid_operation);
        let invalid = indefinite.or(left.signaling_nan.or(&right.signaling_nan));
        let nan = left.nan.or(&right.nan);

        // A QNaN wins over an SNaN. Within either class the larger significand
        // wins; an equal-significand tie keeps the destination as local policy.
        let left_nan = left.nan.and(
            right
                .nan
                .eq(false)
                .or(left.bits.significand.unsigned().ge(&right.bits.significand)),
        );
        let nan_significand = left_nan.select(&left.bits.significand, &right.bits.significand);
        let nan_sign = left_nan.select(&left.bits.sign_exponent, &right.bits.sign_exponent);
        let special = RoundedValue {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand: nan_significand.or(1_u64 << 62),
                sign_exponent: nan_sign,
            })
            .or_indefinite(&indefinite),
            inexact: false.into(),
            incremented: false.into(),
        };
        result.replace_when(&nan.or(&indefinite), &special);
        result.zero_divide = invalid.or(&nan).eq(false).and(&result.zero_divide);
        result.denormal = invalid
            .or(nan)
            .or(&result.zero_divide)
            .eq(false)
            .and(left.denormal.or(&right.denormal));
        result.invalid = invalid;
        result
    }
}
