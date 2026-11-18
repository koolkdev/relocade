//! Binary32 stores use native demotion and compare the result to recover rounding evidence.

use relocade_compiler::{Val, F64, I1, I32, I64};

use super::{ExtendedBits, ExtendedValue, RoundingMode, StoredMagnitude};

pub(super) fn round(
    value: &ExtendedValue,
    bits: &ExtendedBits,
    negative: &Val<I1>,
    rounding: &RoundingMode,
) -> StoredMagnitude {
    let magnitude = match value.binary_value() {
        Some(native) => native.value.abs(),
        None => round_to_odd(bits),
    };
    let nearest_bits = magnitude.to_binary32_bits();
    let nearest_value = Val::<F64>::from_binary32_bits(&nearest_bits);
    let inexact = nearest_value.ne(&magnitude);
    let nearest = rounding.is_nearest();
    let outward = rounding.select(
        false.into(),
        negative.clone(),
        negative.eq(false),
        false.into(),
    );
    let above = nearest_value.gt(&magnitude);
    // Adjacent nonnegative binary32 values have consecutive encodings, including
    // zero and the transition from maximum finite to infinity.
    let increase = outward.and(nearest_value.lt(&magnitude));
    let decrease = nearest.or(&outward).eq(false).and(&above);
    let bits = nearest_bits
        .add(increase.unsigned().extend::<I32>())
        .sub(decrease.unsigned().extend::<I32>());
    let incremented = nearest.select(above, outward.and(&inexact));

    // x87 tests the exponent after precision rounding on an unbounded grid.
    // Just below the minimum normal, that grid's ULP is half a binary32 subnormal ULP.
    let min_normal = f64::from(f32::MIN_POSITIVE);
    let ulp_below_min = f64::from(f32::from_bits(1)) * 0.5;
    let tiny = magnitude.ne(0.0).and(nearest.select(
        magnitude.lt(min_normal - 0.5 * ulp_below_min),
        outward.select(
            magnitude.le(min_normal - ulp_below_min),
            magnitude.lt(min_normal),
        ),
    ));
    let overflow = nearest.select(
        nearest_bits.eq(0x7f80_0000_u32),
        outward.select(
            magnitude.gt(f64::from(f32::MAX)),
            magnitude.ge(2.0_f64.powi(128)),
        ),
    );
    StoredMagnitude {
        bits: bits.unsigned().extend::<I64>(),
        overflow,
        tiny,
        inexact,
        incremented,
    }
}

/// Preserve each finite magnitude's ordering relative to binary32 values,
/// midpoints, and the unbounded-exponent thresholds above. All those boundaries
/// have an even low bit at 53-bit precision. Discarding 11 bits and jamming
/// their OR into the low bit therefore cannot land on or cross a boundary.
fn round_to_odd(bits: &ExtendedBits) -> Val<F64> {
    let exponent = bits.exponent_field();
    // Exponents outside binary64 can be clamped far beyond the binary32 range.
    // Even the smallest nonzero extended subnormal stays on the positive side of zero.
    let native_exponent = exponent.unsigned().lt(16383 - 1022).select(
        1_u32,
        exponent
            .unsigned()
            .ge(16383 + 1023)
            .select(2046_u32, exponent.sub(16383 - 1023)),
    );
    let discarded = bits.significand.and(0x7ff_u64).ne(0_u64);
    let fraction = bits
        .significand
        .unsigned()
        .shr(11)
        .and((1_u64 << 52) - 1)
        .or(discarded.unsigned().extend::<I64>());
    let encoding = native_exponent
        .unsigned()
        .extend::<I64>()
        .shl(52)
        .or(fraction);
    Val::<F64>::from_bits(bits.significand.eq(0_u64).select(0_u64, encoding))
}
