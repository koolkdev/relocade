//! Exact rounding direction and a cheaper precision proof for nearest mode.

use super::super::rounding::RoundedFloat;
use super::*;

pub(super) struct RoundingError {
    pub(super) above: Val<V128>,
    pub(super) below: Val<V128>,
    pub(super) nearest_inexact: Val<V128>,
}

impl RoundingError {
    pub(super) fn round(
        self,
        format: BinaryFormat,
        nearest: Val<V128>,
        mode: &RoundingMode,
    ) -> FloatResult<V128> {
        let mut result = RoundedFloat {
            format,
            nearest,
            above: self.above,
            below: self.below,
        }
        .round(mode);
        // Nearest mode needs only exactness, so its smaller proof survives even
        // when the directed correction would require the full signed residual.
        result.exceptions.precision = mode.is_nearest().select(
            self.nearest_inexact.bitmask::<I32>().ne(0),
            result.exceptions.precision,
        );
        result
    }
}

impl PackedFloatOperand {
    pub(super) fn arithmetic_error(
        &self,
        right: &Val<V128>,
        nearest: &Val<V128>,
        operation: FloatArithmetic,
    ) -> RoundingError {
        let left = &self.bits;
        macro_rules! calculate {
            ($view:ident) => {{
                match operation {
                    FloatArithmetic::Add | FloatArithmetic::Subtract => {
                        let addend = if matches!(operation, FloatArithmetic::Subtract) {
                            right.$view().neg()
                        } else {
                            right.clone()
                        };
                        let left_larger = left.$view().abs().$view().ge(addend.$view().abs());
                        let larger = left_larger.select_bits(left, &addend);
                        let smaller = left_larger.select_bits(addend, left);
                        // FastTwoSum returns exact-minus-nearest, including a subnormal error.
                        let error = smaller.$view().sub(nearest.$view().sub(larger));
                        RoundingError {
                            above: error.$view().lt(0_u128),
                            below: error.$view().gt(0_u128),
                            nearest_inexact: error.$view().ne(0_u128),
                        }
                    }
                    FloatArithmetic::Multiply => self.product_error(left, right, nearest),
                    FloatArithmetic::Divide => {
                        let product = nearest.$view().mul(right);
                        let error = self.product_error(nearest, right, &product);
                        let equal = product.$view().eq(left);
                        // If the rounded multiply-back differs, monotonic rounding
                        // proves the direction. Equality needs the exact product error.
                        let greater = product.$view().gt(left).or(equal.and(error.below));
                        let less = product.$view().lt(left).or(equal.and(error.above));
                        let divisor_negative = right.$view().lt(0_u128);
                        RoundingError {
                            above: divisor_negative.select_bits(&less, &greater),
                            below: divisor_negative.select_bits(greater, less),
                            nearest_inexact: product.$view().ne(left).or(error.nearest_inexact),
                        }
                    }
                }
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4),
            BinaryFormat::Binary64 => calculate!(f64x2),
        }
    }

    pub(super) fn square_root_error(&self, nearest: &Val<V128>) -> RoundingError {
        let source = &self.bits;
        macro_rules! calculate {
            ($view:ident) => {{
                let product = nearest.$view().mul(nearest);
                let error = self.product_error(nearest, nearest, &product);
                let equal = product.$view().eq(source);
                RoundingError {
                    above: product.$view().gt(source).or(equal.and(error.below)),
                    below: product.$view().lt(source).or(equal.and(error.above)),
                    nearest_inexact: product.$view().ne(source).or(error.nearest_inexact),
                }
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4),
            BinaryFormat::Binary64 => calculate!(f64x2),
        }
    }
}
