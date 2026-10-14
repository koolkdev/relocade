//! Native signed conversions retain x86 validity and precision evidence.

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
    let negative = source.signed_shr_lanes::<I32>(31);
    let step_down = negative.select_bits(
        Val::<V128>::splat::<I32>(1_u32),
        Val::<V128>::splat::<I32>(u32::MAX),
    );
    let down = nearest.add_lanes::<I32>(above.and(&step_down));
    let up = nearest.sub_lanes::<I32>(below.and(step_down));
    let toward_zero = negative.select_bits(&up, &down);
    FloatResult {
        value: rounding.select(nearest, down, up, toward_zero),
        exceptions: SimdExceptions {
            precision: above.or(below).bitmask::<I32>().ne(0),
            ..SimdExceptions::default()
        },
    }
}
