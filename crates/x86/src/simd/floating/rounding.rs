//! MXCSR determines how a rounded magnitude reports range and precision.

use super::{FloatResult, SimdExceptions};
use crate::{
    floating::{BinaryEncoding, FiniteMagnitude},
    state::Mxcsr,
};
use wasm86_compiler::{AtLeast, Val, I1, I64};

impl FiniteMagnitude {
    /// Apply FTZ and range-exception policy to an unsigned encoding. The SIMD
    /// operation chooses special responses and inserts its result sign afterward.
    /// Nonfinite inputs suppress exceptions; their numerical candidate is unused.
    pub(crate) fn round_simd_magnitude<T: BinaryEncoding>(
        &self,
        controls: &Mxcsr,
        finite: &Val<I1>,
    ) -> FloatResult<T>
    where
        I64: AtLeast<T>,
    {
        let rounding = controls.rounding();
        let rounded = self.round_binary::<T>(&rounding, finite);
        let overflow = rounded.overflow;
        let tiny = rounded.tiny;
        let flush = tiny
            .and(controls.underflow_masked())
            .and(controls.flush_to_zero());
        let magnitude = overflow.select(
            rounding.overflow_magnitude::<T>(&self.negative),
            rounded.magnitude,
        );
        let unmasked_range = overflow
            .and(controls.overflow_masked().eq(false))
            .or(tiny.and(controls.underflow_masked().eq(false)));
        FloatResult {
            value: flush.select(0, magnitude),
            exceptions: SimdExceptions {
                overflow: overflow.clone(),
                underflow: tiny.and(
                    controls
                        .underflow_masked()
                        .eq(false)
                        .or(controls.flush_to_zero())
                        .or(&rounded.bounded_inexact),
                ),
                precision: finite.and(unmasked_range.select(
                    rounded.unbounded_inexact,
                    overflow.or(&rounded.bounded_inexact).or(flush),
                )),
                ..SimdExceptions::default()
            },
        }
    }
}

impl<T: BinaryEncoding> FloatResult<T>
where
    I64: AtLeast<T>,
{
    /// A special response replaces the numerical result and its range/precision
    /// evidence. Operand exception conditions are supplied by the caller.
    pub(super) fn replace_exact(&mut self, condition: &Val<I1>, bits: Val<I64>) {
        self.value = condition.select(bits.truncate::<T>(), &self.value);
        self.exceptions.overflow = condition.eq(false).and(&self.exceptions.overflow);
        self.exceptions.underflow = condition.eq(false).and(&self.exceptions.underflow);
        self.exceptions.precision = condition.eq(false).and(&self.exceptions.precision);
    }
}
