//! Low significand-product bits prove exactness without a wide integer product.

use super::*;

impl PackedFloatOperand {
    /// Normal nonzero inputs and a normal rounded product discard p-1 or p
    /// product bits, where p is the significand width. A carry caused by rounding
    /// already has nonzero low p-1 bits, so the rounded exponent can choose the
    /// mask. Low lane multiplication retains every bit that this test needs.
    ///
    /// Divide and sqrt use this only to disprove an otherwise equal multiply-back:
    /// equality with their admitted normal input establishes the product domain.
    pub(super) fn product_inexact(
        &self,
        left: &Val<V128>,
        right: &Val<V128>,
        result: &Val<V128>,
    ) -> Val<V128> {
        macro_rules! calculate {
            ($view:ident, $lane:ty, $scalar:ty) => {{
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
                let carry = exponent(result).eq_lanes::<$lane>(carried_exponent);
                let lost = product
                    .and(fraction.or(carry.and(hidden)))
                    .eq_lanes::<$lane>(0_u128)
                    .xor(u128::MAX);
                lost.and(left.$view().ne(0_u128))
                    .and(right.$view().ne(0_u128))
            }};
        }
        match self.format {
            BinaryFormat::Binary32 => calculate!(f32x4, I32, u32),
            BinaryFormat::Binary64 => calculate!(f64x2, I64, u64),
        }
    }
}
