//! Packed word products share the MMX binary access and state contract.

use super::*;

instruction_families! {
    PMULLW {
        execute: packed_binary::<_>(|left, right| left.mul_lanes::<I16>(right));
        forms { NP 0x0f 0xd5 => qword(mm, mm_rm); }
    }
    PMULHW {
        execute: packed_binary::<_>(high_products);
        forms { NP 0x0f 0xe5 => qword(mm, mm_rm); }
    }
    PMADDWD {
        execute: packed_binary::<_>(|left, right| left.signed_dot_i16_pairs(right));
        forms { NP 0x0f 0xf5 => qword(mm, mm_rm); }
    }
}

fn high_products(left: Val<V128>, right: Val<V128>) -> Val<V128> {
    // The high halves of signed word products fit in signed words, so narrowing
    // after the shift is exact. Only the four low input lanes belong to MMX.
    left.signed_widening_mul_low_lanes::<I16>(right)
        .signed_shr_lanes::<I32>(16)
        .signed_narrow_lanes::<I16>(0_u128)
}
