//! Packed word products share the MMX binary access and state contract.

use super::*;

instruction_families! {
    PMULLW {
        execute: binary::<_>(|left, right| left.mul_lanes::<I16>(right));
        forms { NP 0x0f 0xd5 => qword(mm, mm_rm); }
    }
    PMULHW {
        execute: binary::<_>(high_products);
        forms { NP 0x0f 0xe5 => qword(mm, mm_rm); }
    }
    PMADDWD {
        execute: binary::<_>(|left, right| left.signed_dot_i16_pairs(right));
        forms { NP 0x0f 0xf5 => qword(mm, mm_rm); }
    }
}

fn high_products(left: Val<V64>, right: Val<V64>) -> Val<V64> {
    // The high halves of signed word products fit in signed words, so narrowing
    // after the shift is exact. Widening all four words uses a full V128 result.
    left.signed_widening_mul_lanes::<I16>(right)
        .signed_shr_lanes::<I32>(16)
        .signed_narrow_lanes::<I16>(0_u128)
        .low_half()
}
