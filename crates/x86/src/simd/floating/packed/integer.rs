//! Native signed conversions retain x86 validity and precision evidence.

use super::rounding::RoundedBinary32;
use super::*;

impl PackedFloatOperand {
    pub(crate) fn to_integer(&self, rounding: &RoundingMode) -> FloatResult<V128> {
        let (value, valid, inexact) = match self.format {
            BinaryFormat::Binary32 => {
                let lanes = self.bits.f32x4();
                let rounded =
                    rounding.select(lanes.nearest(), lanes.floor(), lanes.ceil(), lanes.trunc());
                let valid = rounded
                    .f32x4()
                    .ge(Val::<V128>::splat::<I32>(0xcf00_0000_u32))
                    .and(
                        rounded
                            .f32x4()
                            .lt(Val::<V128>::splat::<I32>(0x4f00_0000_u32)),
                    );
                let inexact = self.bits.f32x4().ne(&rounded).and(&valid);
                let value = valid.select_bits(
                    rounded.f32x4().trunc_saturating_i32(),
                    Val::<V128>::splat::<I32>(0x8000_0000_u32),
                );
                (value, valid, inexact)
            }
            BinaryFormat::Binary64 => {
                let lanes = self.bits.f64x2();
                let rounded =
                    rounding.select(lanes.nearest(), lanes.floor(), lanes.ceil(), lanes.trunc());
                let valid = rounded
                    .f64x2()
                    .ge(Val::<V128>::splat::<I64>(0xc1e0_0000_0000_0000_u64))
                    .and(
                        rounded
                            .f64x2()
                            .lt(Val::<V128>::splat::<I64>(0x41e0_0000_0000_0000_u64)),
                    );
                let inexact = self.bits.f64x2().ne(&rounded).and(&valid);
                let mask = valid.shuffle_bytes(
                    0_u128,
                    [0, 1, 2, 3, 8, 9, 10, 11, 16, 16, 16, 16, 16, 16, 16, 16],
                );
                let value = mask.select_bits(
                    rounded.f64x2().trunc_saturating_i32_zero(),
                    0x8000_0000_8000_0000_u128,
                );
                (value, valid, inexact)
            }
        };
        FloatResult {
            value,
            exceptions: SimdExceptions {
                invalid: valid.bitmask::<I32>().ne(15),
                precision: inexact.bitmask::<I32>().ne(0),
                // These conversions honor DAZ without raising denormal operand.
                ..SimdExceptions::default()
            },
        }
    }
}

pub(crate) fn packed_integer_to_float(
    format: BinaryFormat,
    source: Val<V128>,
    rounding: &RoundingMode,
) -> FloatResult<V128> {
    if format == BinaryFormat::Binary64 {
        return FloatResult {
            value: source.low_signed_i32_lanes_to_f64(),
            exceptions: SimdExceptions::default(),
        };
    }
    let nearest = source.signed_i32_lanes_to_f32();
    let restored = nearest.f32x4().trunc_saturating_i32();
    // MAX rounds to 2^31, then saturates back to MAX. Correct that one false
    // equality before using the integer round trip as precision evidence.
    let above = restored
        .signed_gt_lanes::<I32>(&source)
        .or(source.eq_lanes::<I32>(Val::<V128>::splat::<I32>(i32::MAX)));
    let below = source.signed_gt_lanes::<I32>(&restored);
    RoundedBinary32 {
        nearest,
        above,
        below,
    }
    .round(rounding)
}
