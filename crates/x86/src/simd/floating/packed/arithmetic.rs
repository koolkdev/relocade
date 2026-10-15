//! Native arithmetic with exact rounding evidence on the normal domain.

use super::*;
use crate::simd::FloatArithmetic;

mod product;
mod rounding;

use rounding::RoundingError;

impl PackedFloatOperand {
    pub(crate) fn arithmetic(
        &self,
        right: &Self,
        operation: FloatArithmetic,
        rounding: &RoundingMode,
    ) -> PackedFloatCandidate {
        let left = &self.bits;
        let right = &right.bits;
        macro_rules! calculate {
            ($view:ident, $lane:ty, $scalar:ty) => {{
                let nearest = match operation {
                    FloatArithmetic::Add => left.$view().add(right),
                    FloatArithmetic::Subtract => left.$view().sub(right),
                    FloatArithmetic::Multiply => left.$view().mul(right),
                    FloatArithmetic::Divide => left.$view().div(right),
                };
                let zero = match operation {
                    FloatArithmetic::Add | FloatArithmetic::Subtract => nearest.$view().eq(0_u128),
                    FloatArithmetic::Multiply => {
                        left.$view().eq(0_u128).or(right.$view().eq(0_u128))
                    }
                    FloatArithmetic::Divide => left.$view().eq(0_u128),
                };
                let mut valid = self.normal_or_zero(left).and(self.normal_or_zero(right));
                if matches!(operation, FloatArithmetic::Divide) {
                    valid = valid.and(right.$view().ne(0_u128));
                }
                let mut result = self.arithmetic_error(right, &nearest, operation).round(
                    self.format,
                    nearest.clone(),
                    rounding,
                );
                if matches!(operation, FloatArithmetic::Add | FloatArithmetic::Subtract) {
                    let addend = if matches!(operation, FloatArithmetic::Subtract) {
                        right.$view().neg()
                    } else {
                        right.clone()
                    };
                    let sign = Val::<V128>::splat::<$lane>(self.format.sign_bit() as $scalar);
                    let cancellation =
                        zero.and(left.xor(addend).and(&sign).eq_lanes::<$lane>(sign.clone()));
                    // Like-signed zero sums retain their sign. Only cancellation
                    // changes from nearest's +0 to -0 when rounding down.
                    result.value = rounding
                        .cancellation_negative()
                        .select(cancellation.select_bits(sign, &result.value), &result.value);
                }
                self.candidate(nearest, valid, zero, result)
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4, I32, u32),
            BinaryFormat::Binary64 => calculate!(f64x2, I64, u64),
        }
    }

    pub(crate) fn square_root(&self, rounding: &RoundingMode) -> PackedFloatCandidate {
        let source = &self.bits;
        macro_rules! calculate {
            ($view:ident) => {{
                let nearest = source.$view().sqrt();
                let result =
                    self.square_root_error(&nearest)
                        .round(self.format, nearest.clone(), rounding);
                let valid = self.normal_or_zero(source).and(source.$view().ge(0_u128));
                self.candidate(nearest, valid, source.$view().eq(0_u128), result)
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4),
            BinaryFormat::Binary64 => calculate!(f64x2),
        }
    }

    fn normal_or_zero(&self, bits: &Val<V128>) -> Val<V128> {
        macro_rules! classify {
            ($view:ident, $lane:ty, $scalar:ty) => {{
                let absolute = bits.$view().abs();
                absolute
                    .$view()
                    .ge(Val::<V128>::splat::<$lane>(
                        (1_u64 << self.format.fraction_bits()) as $scalar,
                    ))
                    .and(absolute.$view().lt(Val::<V128>::splat::<$lane>(
                        self.format.infinity() as $scalar
                    )))
                    .or(absolute.$view().eq(0_u128))
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => classify!(f32x4, I32, u32),
            BinaryFormat::Binary64 => classify!(f64x2, I64, u64),
        }
    }

    fn candidate(
        &self,
        nearest: Val<V128>,
        inputs_valid: Val<V128>,
        exact_zero: Val<V128>,
        result: FloatResult<V128>,
    ) -> PackedFloatCandidate {
        macro_rules! range {
            ($view:ident, $lane:ty, $scalar:ty) => {{
                let absolute = nearest.$view().abs();
                // Exclude the minimum-normal boundary: a tiny unbounded result
                // can round up to it. Range-edge behavior stays in the interpreter.
                absolute
                    .$view()
                    .gt(Val::<V128>::splat::<$lane>(
                        (1_u64 << self.format.fraction_bits()) as $scalar,
                    ))
                    .and(absolute.$view().lt(Val::<V128>::splat::<$lane>(
                        (self.format.infinity() - 1) as $scalar,
                    )))
                    .or(exact_zero)
            }};
        }
        let range = match self.format {
            BinaryFormat::Binary32 => range!(f32x4, I32, u32),
            BinaryFormat::Binary64 => range!(f64x2, I64, u64),
        };
        PackedFloatCandidate {
            valid: inputs_valid.and(range).bitmask::<I32>().eq(15),
            result,
        }
    }
}
