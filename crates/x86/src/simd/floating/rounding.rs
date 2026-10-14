//! MXCSR determines how a rounded finite result reports range and precision.

use super::{FloatLane, FloatResult, SimdExceptions};
use crate::{
    floating::{BinaryRounded, FiniteMagnitude},
    state::Mxcsr,
};
use wasm86_compiler::{AtLeast, Val, I1, I64};

impl FiniteMagnitude {
    pub(crate) fn round_simd<T: FloatLane>(
        &self,
        controls: &Mxcsr,
        finite: &Val<I1>,
    ) -> FloatResult<T>
    where
        I64: AtLeast<T>,
    {
        let BinaryRounded {
            bits,
            overflow,
            tiny,
            inexact,
            unbounded_inexact,
            ..
        } = self.round_binary(T::FORMAT, &controls.rounding(), finite);
        let flush = tiny
            .and(controls.underflow_masked())
            .and(controls.flush_to_zero());
        let unmasked_range = overflow
            .and(controls.overflow_masked().eq(false))
            .or(tiny.and(controls.underflow_masked().eq(false)));
        FloatResult {
            value: flush
                .select(bits.and(T::FORMAT.sign_bit()), bits)
                .truncate::<T>(),
            exceptions: SimdExceptions {
                overflow,
                underflow: tiny.and(
                    controls
                        .underflow_masked()
                        .eq(false)
                        .or(controls.flush_to_zero())
                        .or(&inexact),
                ),
                precision: finite.and(unmasked_range.select(unbounded_inexact, inexact.or(flush))),
                ..SimdExceptions::default()
            },
        }
    }
}
