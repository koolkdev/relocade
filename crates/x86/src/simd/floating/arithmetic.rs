//! Arithmetic preserves source NaN priority and reports each lane before publication.

use super::{FloatOperand, FloatResult};
use crate::{
    floating::{BinaryClassification, BinaryEncoding},
    state::Mxcsr,
};
use wasm86_compiler::{AtLeast, BlockBuilder, BuildError, Val, I1, I64};

#[derive(Clone, Copy)]
pub(crate) enum FloatArithmetic {
    Add,
    Subtract,
    Multiply,
}

pub(crate) struct ArithmeticOperands<T: BinaryEncoding> {
    left: FloatOperand<T>,
    right: FloatOperand<T>,
}

impl<T: BinaryEncoding> ArithmeticOperands<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn new(left: Val<T>, right: Val<T>, daz: Val<I1>) -> Self {
        Self {
            left: FloatOperand::new(left, daz.clone()),
            right: FloatOperand::new(right, daz),
        }
    }

    pub(crate) fn calculate(
        &self,
        body: &mut BlockBuilder<'_>,
        operation: FloatArithmetic,
        controls: &Mxcsr,
    ) -> Result<FloatResult<T>, BuildError> {
        let left_bits = self.left.bits.unsigned().extend::<I64>();
        let right_bits = self.right.bits.unsigned().extend::<I64>();
        let left = T::FORMAT.classification(&left_bits);
        let right = T::FORMAT.classification(&right_bits);
        let left_input = T::FORMAT.finite_input(&left_bits);
        let mut right_input = T::FORMAT.finite_input(&right_bits);
        right_input.negative = right_input
            .negative
            .xor(matches!(operation, FloatArithmetic::Subtract));
        let magnitude = match operation {
            FloatArithmetic::Add | FloatArithmetic::Subtract => {
                left_input.add(body, &right_input, &controls.rounding())?
            }
            FloatArithmetic::Multiply => left_input.multiply(&right_input),
        };
        let mut result =
            magnitude.round_simd_magnitude::<T>(controls, &left.finite().and(right.finite()));
        let sign = magnitude
            .negative
            .select::<I64>(T::FORMAT.sign_bit(), 0_u64);
        result.value = result.value.or(sign.truncate::<T>());
        let infinity = left.infinity().or(right.infinity());
        let invalid_operation = match operation {
            FloatArithmetic::Add | FloatArithmetic::Subtract => {
                Self::replace_exact(&mut result, &magnitude.significand.integer.eq(0_u64), sign);
                let opposite = left_input.negative.xor(&right_input.negative);
                let negative = left
                    .infinity()
                    .select(&left_input.negative, &right_input.negative);
                Self::replace_exact(
                    &mut result,
                    &infinity,
                    negative
                        .select(T::FORMAT.sign_bit(), 0_u64)
                        .or(T::FORMAT.infinity()),
                );
                left.infinity().and(right.infinity()).and(opposite)
            }
            FloatArithmetic::Multiply => {
                let negative = left_input.negative.xor(&right_input.negative);
                let zero = left.zero().or(right.zero());
                let sign = negative.select(T::FORMAT.sign_bit(), 0_u64);
                Self::replace_exact(&mut result, &zero, sign.clone());
                Self::replace_exact(&mut result, &infinity, sign.or(T::FORMAT.infinity()));
                infinity.and(zero)
            }
        };
        Ok(self.finish(result, &left, &right, invalid_operation))
    }

    fn replace_exact(result: &mut FloatResult<T>, condition: &Val<I1>, bits: Val<I64>) {
        result.value = condition.select(bits.truncate::<T>(), &result.value);
        result.exceptions.overflow = condition.eq(false).and(&result.exceptions.overflow);
        result.exceptions.underflow = condition.eq(false).and(&result.exceptions.underflow);
        result.exceptions.precision = condition.eq(false).and(&result.exceptions.precision);
    }

    fn finish(
        &self,
        mut result: FloatResult<T>,
        left: &BinaryClassification<'_>,
        right: &BinaryClassification<'_>,
        invalid_operation: Val<I1>,
    ) -> FloatResult<T> {
        let nan = left.nan().or(right.nan());
        let invalid = left
            .signaling_nan()
            .or(right.signaling_nan())
            .or(&invalid_operation);
        // SSE selects the first NaN in destination/source order and quiets it.
        let nan_bits = left
            .nan()
            .select(&self.left.bits, &self.right.bits)
            .unsigned()
            .extend::<I64>()
            .or(T::FORMAT.quiet_bit());
        Self::replace_exact(
            &mut result,
            &nan.or(&invalid_operation),
            nan.select(nan_bits, T::FORMAT.indefinite_bits()),
        );
        result.exceptions.invalid = invalid.clone();
        result.exceptions.denormal = invalid
            .or(nan)
            .eq(false)
            .and(self.left.denormal.or(&self.right.denormal));
        result
    }
}
