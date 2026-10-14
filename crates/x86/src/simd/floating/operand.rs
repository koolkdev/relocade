//! One SIMD floating operand retains its encoding after DAZ normalization.

use crate::floating::{BinaryClassification, BinaryEncoding};
use wasm86_compiler::{AtLeast, Val, F64, I1, I64};

pub(crate) struct FloatOperand<T: BinaryEncoding> {
    original_bits: Val<I64>,
    pub(crate) bits: Val<T>,
    pub(super) denormal: Val<I1>,
}

impl<T: BinaryEncoding> FloatOperand<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn new(bits: Val<T>, daz: Val<I1>) -> Self {
        let original_bits = bits.unsigned().extend::<I64>();
        let denormal = T::FORMAT.classification(&original_bits).denormal();
        let sign = Val::<I64>::from(T::FORMAT.sign_bit()).truncate::<T>();
        Self {
            original_bits,
            bits: daz.and(&denormal).select(bits.and(sign), bits),
            denormal: daz.eq(false).and(denormal),
        }
    }

    pub(crate) fn classification(&self) -> BinaryClassification<'_> {
        // Classify before DAZ selection so NaN checks do not depend on the
        // normalized value. Denormal reporting separately accounts for DAZ.
        T::FORMAT.classification(&self.original_bits)
    }

    pub(crate) fn value(&self) -> Val<F64> {
        T::FORMAT.binary64_value(&self.bits.unsigned().extend::<I64>())
    }
}
