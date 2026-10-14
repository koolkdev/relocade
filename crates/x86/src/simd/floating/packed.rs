//! Packed IEEE encodings retain lane masks for DAZ and exception aggregation.

mod integer;

use super::{FloatResult, SimdExceptions};
use crate::floating::{BinaryFormat, RoundingMode};
use wasm86_compiler::{Val, I1, I32, I64, I8, V128};

pub(crate) use integer::packed_integer_to_float;

pub(crate) struct PackedFloatOperand {
    format: BinaryFormat,
    original: Val<V128>,
    bits: Val<V128>,
    denormal: Val<V128>,
}

impl PackedFloatOperand {
    pub(crate) fn new(format: BinaryFormat, original: Val<V128>, daz: Val<I1>) -> Self {
        let (sign, denormal) = match format {
            BinaryFormat::Binary32 => {
                let magnitude = original.and(Val::<V128>::splat::<I32>(0x7fff_ffff_u32));
                let denormal = Val::<V128>::splat::<I32>(0x0080_0000_u32)
                    .signed_gt_lanes::<I32>(&magnitude)
                    .and(magnitude.eq_lanes::<I32>(0_u128).xor(u128::MAX));
                (Val::<V128>::splat::<I32>(0x8000_0000_u32), denormal)
            }
            BinaryFormat::Binary64 => {
                let magnitude = original.and(Val::<V128>::splat::<I64>(0x7fff_ffff_ffff_ffff_u64));
                let denormal = Val::<V128>::splat::<I64>(0x0010_0000_0000_0000_u64)
                    .signed_gt_lanes::<I64>(&magnitude)
                    .and(magnitude.eq_lanes::<I64>(0_u128).xor(u128::MAX));
                (
                    Val::<V128>::splat::<I64>(0x8000_0000_0000_0000_u64),
                    denormal,
                )
            }
        };
        let flushed = denormal.select_bits(original.and(sign), &original);
        Self {
            format,
            bits: daz.select(flushed, &original),
            original,
            denormal: daz.select(0_u128, denormal),
        }
    }

    fn nan(&self) -> Val<V128> {
        match self.format {
            BinaryFormat::Binary32 => self
                .original
                .f32x4()
                .abs()
                .signed_gt_lanes::<I32>(Val::<V128>::splat::<I32>(0x7f80_0000_u32)),
            BinaryFormat::Binary64 => self
                .original
                .f64x2()
                .abs()
                .signed_gt_lanes::<I64>(Val::<V128>::splat::<I64>(0x7ff0_0000_0000_0000_u64)),
        }
    }

    fn signaling_nan(&self) -> Val<V128> {
        let quiet_clear = match self.format {
            BinaryFormat::Binary32 => self
                .original
                .and(Val::<V128>::splat::<I32>(0x0040_0000_u32))
                .eq_lanes::<I32>(0_u128),
            BinaryFormat::Binary64 => self
                .original
                .and(Val::<V128>::splat::<I64>(0x0008_0000_0000_0000_u64))
                .eq_lanes::<I64>(0_u128),
        };
        self.nan().and(quiet_clear)
    }

    /// Negated legacy predicates include unordered pairs. Exception masks are
    /// reduced only after NaN precedence has been applied separately per lane.
    pub(crate) fn compare_mask(&self, right: &Self, predicate: Val<I8>) -> FloatResult<V128> {
        let kind = predicate.and(3);
        let unordered = self.nan().or(right.nan());
        let (equal, less, less_equal) = match self.format {
            BinaryFormat::Binary32 => (
                self.bits.f32x4().eq(&right.bits),
                self.bits.f32x4().lt(&right.bits),
                self.bits.f32x4().le(&right.bits),
            ),
            BinaryFormat::Binary64 => (
                self.bits.f64x2().eq(&right.bits),
                self.bits.f64x2().lt(&right.bits),
                self.bits.f64x2().le(&right.bits),
            ),
        };
        let matches = kind.eq(0).select(
            equal,
            kind.eq(1)
                .select(less, kind.eq(2).select(less_equal, &unordered)),
        );
        let signaling = self.signaling_nan().or(right.signaling_nan());
        let invalid = kind.eq(1).or(kind.eq(2)).select(&unordered, signaling);
        FloatResult {
            value: predicate
                .and(4)
                .ne(0)
                .select(matches.xor(u128::MAX), matches),
            exceptions: SimdExceptions {
                invalid: invalid.bitmask::<I32>().ne(0),
                denormal: self
                    .denormal
                    .or(&right.denormal)
                    .and(unordered.xor(u128::MAX))
                    .bitmask::<I32>()
                    .ne(0),
                ..SimdExceptions::default()
            },
        }
    }
}
