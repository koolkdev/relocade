//! Packed width conversions admit native results with exact rounding evidence.

use super::rounding::RoundedFloat;
use super::*;

pub(crate) fn widen_packed_float(source: Val<V128>, daz: Val<I1>) -> PackedFloatCandidate {
    let source = PackedFloatOperand::new(BinaryFormat::Binary32, source, daz);
    // Promotion is exact for every admitted lane. Leave NaN payload handling
    // and reported denormals to the general calculation; upper lanes are unused.
    PackedFloatCandidate {
        valid: source
            .nan()
            .or(&source.denormal)
            .bitmask::<I32>()
            .and(3)
            .eq(0),
        result: FloatResult {
            value: source.bits.f32x4().promote_low(),
            exceptions: SimdExceptions::default(),
        },
    }
}

pub(crate) fn narrow_packed_float(
    source: Val<V128>,
    daz: Val<I1>,
    rounding: &RoundingMode,
) -> PackedFloatCandidate {
    let source = PackedFloatOperand::new(BinaryFormat::Binary64, source, daz);
    let nearest = source.bits.f64x2().demote_zero();
    let restored = nearest.f32x4().promote_low();
    let magnitude = restored.f64x2().abs();
    // Strict bounds put the exact source inside the binary32 normal finite
    // range. A directed neighbor can reach either boundary without crossing it.
    let valid = magnitude
        .f64x2()
        .gt(Val::<V128>::splat::<I64>(
            f64::from(f32::MIN_POSITIVE).to_bits(),
        ))
        .and(
            magnitude
                .f64x2()
                .lt(Val::<V128>::splat::<I64>(f64::from(f32::MAX).to_bits())),
        )
        .or(source.bits.f64x2().eq(0_u128));
    let low_dwords = |mask: Val<V128>| {
        mask.shuffle_bytes(
            0_u128,
            [0, 1, 2, 3, 8, 9, 10, 11, 16, 16, 16, 16, 16, 16, 16, 16],
        )
    };
    PackedFloatCandidate {
        valid: valid.bitmask::<I32>().eq(15),
        result: RoundedFloat {
            format: BinaryFormat::Binary32,
            nearest,
            above: low_dwords(restored.f64x2().gt(&source.bits)),
            below: low_dwords(restored.f64x2().lt(&source.bits)),
        }
        .round(rounding),
    }
}
