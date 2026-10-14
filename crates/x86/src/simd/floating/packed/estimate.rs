//! Reciprocal estimates have fixed rounding and never report numerical exceptions.

use super::*;
use crate::simd::FloatEstimate;

pub(crate) fn estimate_packed_float(source: Val<V128>, operation: FloatEstimate) -> Val<V128> {
    // Both signs of denormal input become zero regardless of MXCSR.DAZ.
    let source = PackedFloatOperand::new(BinaryFormat::Binary32, source, true.into());
    let one = Val::<V128>::splat::<I32>(1.0_f32.to_bits());
    // Strict binary32 division and sqrt are more accurate than the architectural
    // relative-error bound of 1.5 * 2^-12, without a CPU-specific estimate table.
    let result = match operation {
        FloatEstimate::Reciprocal => {
            let result = one.f32x4().div(&source.bits);
            // RCP always flushes tiny outputs, independently of MXCSR.FTZ.
            result
                .and(Val::<V128>::splat::<I32>(0x7f80_0000_u32))
                .eq_lanes::<I32>(0_u128)
                .select_bits(
                    result.and(Val::<V128>::splat::<I32>(0x8000_0000_u32)),
                    &result,
                )
        }
        FloatEstimate::ReciprocalSquareRoot => {
            let result = one.f32x4().div(source.bits.f32x4().sqrt());
            // Negative nonzero inputs produce indefinite. Negative zero still
            // produces negative infinity through native sqrt and division.
            source
                .bits
                .f32x4()
                .lt(0_u128)
                .select_bits(Val::<V128>::splat::<I32>(0xffc0_0000_u32), result)
        }
    };
    source.nan().select_bits(
        source
            .original
            .or(Val::<V128>::splat::<I32>(0x0040_0000_u32)),
        result,
    )
}
