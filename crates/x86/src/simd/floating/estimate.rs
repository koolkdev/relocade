//! Scalar estimates retain compact binary64 arithmetic and x86 special values.

use super::FloatOperand;
use relocade_compiler::{Val, F64, I32};

#[derive(Clone, Copy)]
pub(crate) enum FloatEstimate {
    Reciprocal,
    ReciprocalSquareRoot,
}

impl FloatEstimate {
    pub(crate) fn calculate_scalar(self, bits: Val<I32>) -> Val<I32> {
        // Both signs of denormal input become zero regardless of MXCSR.DAZ.
        let source = FloatOperand::new(bits, true.into());
        let value = source.value();
        let result = match self {
            Self::Reciprocal => {
                let result = Val::<F64>::from(1.0).div(value).to_binary32_bits();
                // RCP always flushes tiny outputs, independently of MXCSR.FTZ.
                result
                    .and(0x7f80_0000)
                    .eq(0)
                    .select(result.and(0x8000_0000_u32), result)
            }
            Self::ReciprocalSquareRoot => {
                let result = Val::<F64>::from(1.0).div(value.sqrt()).to_binary32_bits();
                // Negative zero still produces negative infinity.
                value.lt(0.0).select(0xffc0_0000_u32, result)
            }
        };
        source
            .classification()
            .nan()
            .select(source.bits.or(0x0040_0000), result)
    }
}
