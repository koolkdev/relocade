//! Finite division separates quotient evidence from architectural exceptions.

use super::{quotient, FiniteInput, FiniteMagnitude, QuotientMethod};
use relocade_compiler::I32;

impl FiniteInput {
    /// Zero/special operand responses replace the unused numerical result.
    pub(crate) fn divide(&self, right: &Self, method: QuotientMethod) -> FiniteMagnitude {
        let quotient = quotient::calculate(&self.significand, right.significand.clone(), method);
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
