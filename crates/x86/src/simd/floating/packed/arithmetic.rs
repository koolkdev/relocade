//! Native nearest arithmetic with exact precision evidence on the normal domain.

use super::*;
use crate::simd::FloatArithmetic;

mod precision;

/// The result has only precision effects after every lane's domain is admitted.
pub(crate) struct PackedFloatCandidate {
    pub(crate) valid: Val<I1>,
    pub(crate) result: FloatResult<V128>,
}

impl PackedFloatOperand {
    pub(crate) fn arithmetic_nearest(
        &self,
        right: &Self,
        operation: FloatArithmetic,
    ) -> PackedFloatCandidate {
        let left = &self.bits;
        let right = &right.bits;
        macro_rules! calculate {
            ($view:ident) => {{
                let value = match operation {
                    FloatArithmetic::Add => left.$view().add(right),
                    FloatArithmetic::Subtract => left.$view().sub(right),
                    FloatArithmetic::Multiply => left.$view().mul(right),
                    FloatArithmetic::Divide => left.$view().div(right),
                };
                let zero = match operation {
                    FloatArithmetic::Add | FloatArithmetic::Subtract => value.$view().eq(0_u128),
                    FloatArithmetic::Multiply => {
                        left.$view().eq(0_u128).or(right.$view().eq(0_u128))
                    }
                    FloatArithmetic::Divide => left.$view().eq(0_u128),
                };
                let mut valid = self.normal_or_zero(left).and(self.normal_or_zero(right));
                if matches!(operation, FloatArithmetic::Divide) {
                    valid = valid.and(right.$view().ne(0_u128));
                }
                let precision = match operation {
                    FloatArithmetic::Add | FloatArithmetic::Subtract => {
                        let addend = if matches!(operation, FloatArithmetic::Subtract) {
                            right.$view().neg()
                        } else {
                            right.clone()
                        };
                        let left_larger = left.$view().abs().$view().ge(addend.$view().abs());
                        let larger = left_larger.select_bits(left, &addend);
                        let smaller = left_larger.select_bits(addend, left);
                        // FastTwoSum retains even a subnormal rounding error.
                        smaller
                            .$view()
                            .sub(value.$view().sub(larger))
                            .$view()
                            .ne(0_u128)
                    }
                    FloatArithmetic::Multiply => self.product_inexact(left, right, &value),
                    FloatArithmetic::Divide => {
                        let product = value.$view().mul(right);
                        product
                            .$view()
                            .ne(left)
                            .or(self.product_inexact(&value, right, &product))
                    }
                };
                self.candidate(value, valid, zero, precision)
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4),
            BinaryFormat::Binary64 => calculate!(f64x2),
        }
    }

    pub(crate) fn square_root_nearest(&self) -> PackedFloatCandidate {
        let source = &self.bits;
        macro_rules! calculate {
            ($view:ident) => {{
                let value = source.$view().sqrt();
                let product = value.$view().mul(&value);
                let precision = product
                    .$view()
                    .ne(source)
                    .or(self.product_inexact(&value, &value, &product));
                let valid = self.normal_or_zero(source).and(source.$view().ge(0_u128));
                self.candidate(value, valid, source.$view().eq(0_u128), precision)
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
        value: Val<V128>,
        inputs_valid: Val<V128>,
        exact_zero: Val<V128>,
        precision: Val<V128>,
    ) -> PackedFloatCandidate {
        macro_rules! range {
            ($view:ident, $lane:ty, $scalar:ty) => {{
                let absolute = value.$view().abs();
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
            result: FloatResult {
                value,
                exceptions: SimdExceptions {
                    precision: precision.bitmask::<I32>().ne(0),
                    ..SimdExceptions::default()
                },
            },
        }
    }
}
