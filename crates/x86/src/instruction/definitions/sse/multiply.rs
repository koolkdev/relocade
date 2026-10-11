//! Packed word products fill all eight word lanes or four adjacent-pair sums.

use super::*;

instruction_families! {
    PMULLW {
        execute: binary(|left, right| left.mul_lanes::<I16>(right));
        forms { P66 0x0f 0xd5 => operands(xmm, xmm_rm); }
    }
    PMULHW {
        execute: binary(high_products);
        forms { P66 0x0f 0xe5 => operands(xmm, xmm_rm); }
    }
    PMADDWD {
        execute: binary(|left, right| left.signed_dot_i16_pairs(right));
        forms { P66 0x0f 0xf5 => operands(xmm, xmm_rm); }
    }
}

fn high_products(left: Val<V128>, right: Val<V128>) -> Val<V128> {
    let low = left
        .signed_widening_mul_low_lanes::<I16>(&right)
        .signed_shr_lanes::<I32>(16);
    let high = left
        .signed_widening_mul_high_lanes::<I16>(right)
        .signed_shr_lanes::<I32>(16);
    // Each signed product's high word fits exactly; narrowing only joins the halves.
    low.signed_narrow_lanes::<I16>(high)
}
