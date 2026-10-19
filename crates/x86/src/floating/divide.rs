//! Finite division separates quotient evidence from architectural exceptions.

use super::{quotient, FiniteInput, FiniteMagnitude};
use relocade_compiler::I32;

impl FiniteInput {
    /// Zero/special operand responses replace the unused numerical result.
    /// `divisor_precision53` proves the denominator's low eleven bits are zero.
    pub(crate) fn divide(&self, right: &Self, divisor_precision53: bool) -> FiniteMagnitude {
        let quotient = quotient::calculate(
            &self.significand,
            right.significand.clone(),
            divisor_precision53,
        );
        FiniteMagnitude {
            significand: quotient.significand,
            exponent: self
                .exponent
                .sub(&right.exponent)
                .sub(quotient.below_one.unsigned().extend::<I32>()),
            negative: self.negative.xor(&right.negative),
        }
    }
}
