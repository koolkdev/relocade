//! Signed dword conversions validate the rounded result before publication.

use super::{FloatLane, FloatOperand, FloatResult, SimdExceptions};
use crate::floating::RoundingMode;
use wasm86_compiler::{AtLeast, I32, I64};

impl<T: FloatLane> FloatOperand<T>
where
    I64: AtLeast<T>,
{
    pub(crate) fn to_integer(&self, rounding: &RoundingMode) -> FloatResult<I32> {
        let value = self.value();
        let rounded = rounding.integral(&value);
        // Ordered comparisons reject NaNs and infinities as well as overflow.
        // Validity is determined after rounding, including at the negative bound.
        let valid = rounded.ge(-2147483648.0).and(rounded.lt(2147483648.0));
        FloatResult {
            value: valid.select(rounded.trunc_saturating_i32(), 0x8000_0000_u32),
            exceptions: SimdExceptions {
                invalid: valid.eq(false),
                precision: valid.and(value.ne(rounded)),
                // Integer conversions honor DAZ but never raise denormal operand.
                ..SimdExceptions::default()
            },
        }
    }
}
