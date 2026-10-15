//! Low significand-product bits retain both exactness and signed rounding error.

use super::*;

impl PackedFloatOperand {
    /// Normal nonzero inputs and a normal nearest product discard p-1 or p bits.
    /// The signed residual A*B - C*2^k has magnitude at most 2^(k-1), so wrapping
    /// lane multiplication and subtraction preserve it without a wide product.
    /// Divide and sqrt use its direction only when multiply-back equals their
    /// admitted normal source, establishing this product domain.
    pub(super) fn product_error(
        &self,
        left: &Val<V128>,
        right: &Val<V128>,
        nearest: &Val<V128>,
    ) -> RoundingError {
        macro_rules! calculate {
            ($view:ident, $lane:ty, $scalar:ty, $sign:literal) => {{
                let fraction_bits = self.format.fraction_bits();
                let hidden = Val::<V128>::splat::<$lane>((1_u64 << fraction_bits) as $scalar);
                let fraction =
                    Val::<V128>::splat::<$lane>(((1_u64 << fraction_bits) - 1) as $scalar);
                let exponent_mask = Val::<V128>::splat::<$lane>(
                    ((1_u64 << self.format.exponent_bits()) - 1) as $scalar,
                );
                let exponent = |bits: &Val<V128>| {
                    bits.unsigned_shr_lanes::<$lane>(fraction_bits)
                        .and(&exponent_mask)
                };
                let product = left
                    .and(&fraction)
                    .or(&hidden)
                    .mul_lanes::<$lane>(right.and(&fraction).or(&hidden));
                let carried_exponent = exponent(left)
                    .add_lanes::<$lane>(exponent(right))
                    .sub_lanes::<$lane>(Val::<V128>::splat::<$lane>(
                        (self.format.bias() - 1) as $scalar,
                    ));
                let carry = exponent(nearest).eq_lanes::<$lane>(carried_exponent);
                let nonzero = left.$view().ne(0_u128).and(right.$view().ne(0_u128));
                // Rounding up across a binade already has nonzero low p-1 bits,
                // so the rounded exponent also selects the nearest exactness mask.
                let nearest_inexact = product
                    .and(fraction.or(carry.and(&hidden)))
                    .eq_lanes::<$lane>(0_u128)
                    .xor(u128::MAX)
                    .and(left.$view().ne(0_u128))
                    .and(right.$view().ne(0_u128));
                let significand = nearest.and(&fraction).or(hidden);
                let aligned = carry.select_bits(
                    significand.shl_lanes::<$lane>(fraction_bits + 1),
                    significand.shl_lanes::<$lane>(fraction_bits),
                );
                let residual = product.sub_lanes::<$lane>(aligned);
                let positive = residual.signed_gt_lanes::<$lane>(0_u128);
                let negative = Val::<V128>::from(0_u128).signed_gt_lanes::<$lane>(residual);
                let result_negative = nearest.signed_shr_lanes::<$lane>($sign);
                RoundingError {
                    above: result_negative
                        .select_bits(&positive, &negative)
                        .and(&nonzero),
                    below: result_negative.select_bits(negative, positive).and(nonzero),
                    nearest_inexact,
                }
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4, I32, u32, 31),
            BinaryFormat::Binary64 => calculate!(f64x2, I64, u64, 63),
        }
    }
}
