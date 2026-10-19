//! Width conversions preserve NaN payloads and distinguish precision from range.

use super::{FloatOperand, FloatResult, SimdExceptions};
use crate::{floating::BinaryFormat, state::Mxcsr};
use relocade_compiler::{I32, I64};

impl FloatOperand<I32> {
    pub(crate) fn widen(&self) -> FloatResult<I64> {
        let bits = self.bits.unsigned().extend::<I64>();
        let nan = bits
            .and(0x8000_0000_u64)
            .shl(32)
            .or(bits.and(0x007f_ffff_u64).shl(29))
            .or(0x7ff8_0000_0000_0000_u64);
        FloatResult {
            value: self
                .classification()
                .nan()
                .select(nan, self.value().to_bits()),
            exceptions: SimdExceptions {
                invalid: self.classification().signaling_nan(),
                denormal: self.denormal.clone(),
                ..SimdExceptions::default()
            },
        }
    }
}

impl FloatOperand<I64> {
    pub(crate) fn narrow(&self, controls: &Mxcsr) -> FloatResult<I32> {
        let bits = &self.bits;
        let class = self.classification();
        let finite = BinaryFormat::Binary64.exponent_field(bits).ne(0x7ff);
        let mut result = BinaryFormat::Binary64
            .finite_input(bits)
            .magnitude()
            .round_simd_magnitude::<I32>(controls, &finite);
        let nan = bits
            .unsigned()
            .shr(29)
            .truncate::<I32>()
            .and(0x007f_ffff_u32)
            .or(0x7fc0_0000_u32);
        let sign = bits
            .unsigned()
            .shr(32)
            .truncate::<I32>()
            .and(0x8000_0000_u32);
        result.value = finite
            .eq(false)
            .select(class.nan().select(nan, 0x7f80_0000_u32), result.value)
            .or(sign);
        result.exceptions.invalid = class.signaling_nan();
        result.exceptions.denormal = self.denormal.clone();
        result
    }
}
