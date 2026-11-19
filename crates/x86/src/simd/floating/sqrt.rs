//! Unary square root classifies only the source lane.

use super::{FloatOperand, FloatResult};
use crate::{floating::BinaryEncoding, state::Mxcsr};
use relocade_compiler::{AtLeast, I64};

impl<T: BinaryEncoding> FloatOperand<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn square_root(&self, controls: &Mxcsr) -> FloatResult<T> {
        let bits = self.bits.unsigned().extend::<I64>();
        let class = T::FORMAT.classification(&bits);
        let input = T::FORMAT.finite_input(&bits);
        let magnitude = input.square_root();
        let mut result = magnitude.round_simd_magnitude::<T>(controls, &class.finite());
        let invalid_operation = input.negative.and(class.zero().or(class.nan()).eq(false));
        let special = class
            .zero()
            .or(class.infinity())
            .or(class.nan())
            .or(&invalid_operation);
        let special_bits = class.nan().select(
            bits.or(T::FORMAT.quiet_bit()),
            invalid_operation.select(T::FORMAT.indefinite_bits(), &bits),
        );
        result.replace_exact(&special, special_bits);
        result.exceptions.invalid = class.signaling_nan().or(invalid_operation);
        result.exceptions.denormal = result
            .exceptions
            .invalid
            .or(class.nan())
            .eq(false)
            .and(&self.denormal);
        result
    }
}
