//! Binary operands share normalization, NaN selection and exception priority.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I64, I8};

use super::{
    result::RoundedValue, ArithmeticResult, BinaryArithmetic, BinaryOperation, ExtendedBits,
    ExtendedValue, RoundingMode,
};

pub(super) struct Operand {
    pub(super) bits: ExtendedBits,
    pub(super) normal: Val<I1>,
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
            normal: value.normal(),
            zero: value.zero(),
            infinity: value.infinity(),
            nan: value.nan(),
            signaling_nan: value.signaling_nan(),
            denormal: value.denormal(),
            unsupported: value.unsupported(),
        }
    }

    pub(super) fn normalized(&self) -> (Val<I64>, Val<I32>) {
        // Normal and zero inputs retain their significands. Other classes use
        // denormal normalization; special-operand responses discard that result.
        // The admission predicate also lets an earlier guard remove this work.
        let ordinary = self.normal.or(&self.zero);
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
}

impl BinaryOperands {
    pub(crate) fn new(left: &ExtendedValue, right: &ExtendedValue) -> Self {
        Self {
            left: Operand::new(left),
            right: Operand::new(right),
        }
    }

    pub(crate) fn normal_or_zero(&self) -> Val<I1> {
        self.left
            .normal
            .or(&self.left.zero)
            .and(self.right.normal.or(&self.right.zero))
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
            BinaryOperation::Add | BinaryOperation::Subtract | BinaryOperation::ReverseSubtract => {
                super::add::add(body, self, operation, precision, rounding)
            }
        }
    }

    /// Applies shared operand responses after the operation supplies its invalid
    /// combination and infinity sign. Original NaN signs are never negated by
    /// subtraction, and equal payloads retain the destination operand's sign.
    pub(super) fn finish(
        &self,
        mut result: ArithmeticResult,
        invalid_operation: Val<I1>,
        infinity_negative: Val<I1>,
    ) -> ArithmeticResult {
        let Self { left, right } = self;
        let indefinite = left
            .unsupported
            .or(&right.unsupported)
            .or(invalid_operation);
        let invalid = indefinite.or(left.signaling_nan.or(&right.signaling_nan));
        let nan = left.nan.or(&right.nan);
        let infinity = left.infinity.or(&right.infinity);

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
                significand: nan.select(nan_significand.or(1_u64 << 62), 1_u64 << 63),
                sign_exponent: nan.select(
                    nan_sign,
                    infinity_negative.select::<I16>(0x8000, 0).or(0x7fff),
                ),
            })
            .or_indefinite(&indefinite),
            inexact: false.into(),
            incremented: false.into(),
        };
        result.replace_when(&nan.or(&indefinite).or(infinity), &special);
        result.denormal = invalid
            .or(nan)
            .eq(false)
            .and(left.denormal.or(&right.denormal));
        result.invalid = invalid;
        result
    }
}
