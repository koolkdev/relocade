//! Packed word and dword products share the MMX binary access and state contract.

use super::*;

instruction_families! {
    PMULLW {
        execute: binary::<_>(|left, right| left.mul_lanes::<I16>(right));
        requires: [Mmx];
        forms { NP 0x0f 0xd5 => qword(mm, mm_rm); }
    }
    PMULHW {
        execute: binary::<_>(|left, right| high_products(left, right, true));
        requires: [Mmx];
        forms { NP 0x0f 0xe5 => qword(mm, mm_rm); }
    }
    PMULHUW {
        execute: binary::<_>(|left, right| high_products(left, right, false));
        requires: [Sse];
        forms { NP 0x0f 0xe4 => qword(mm, mm_rm); }
    }
    PMULUDQ {
        execute: binary::<_>(unsigned_dword_products);
        requires: [Sse2];
        forms { NP 0x0f 0xf4 => qword(mm, mm_rm); }
    }
    PMADDWD {
        execute: binary::<_>(|left, right| left.signed_dot_i16_pairs(right));
        requires: [Mmx];
        forms { NP 0x0f 0xf5 => qword(mm, mm_rm); }
    }
}

fn high_products(left: Val<V64>, right: Val<V64>, signed: bool) -> Val<V64> {
    let products = if signed {
        left.signed_widening_mul_lanes::<I16>(right)
    } else {
        left.unsigned_widening_mul_lanes::<I16>(right)
    };
    // Logical shifts put the raw high words in the unsigned narrowing range.
    products
        .unsigned_shr_lanes::<I32>(16)
        .unsigned_narrow_lanes::<I16>(0_u128)
        .low_half()
}

fn unsigned_dword_products(left: Val<V64>, right: Val<V64>) -> Val<V64> {
    // Only the low dword contributes to the MMX form's single qword result.
    left.unsigned_widening_mul_lanes::<I32>(right).low_half()
}
