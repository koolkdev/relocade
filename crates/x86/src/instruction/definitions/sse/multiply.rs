//! Packed word and dword products preserve the selected signedness and lane order.

use super::*;

instruction_families! {
    PMULLW {
        execute: binary(|left, right| left.mul_lanes::<I16>(right));
        forms { P66 0x0f 0xd5 => operands(xmm, xmm_rm); }
    }
    PMULHW {
        execute: binary(|left, right| high_products(left, right, true));
        forms { P66 0x0f 0xe5 => operands(xmm, xmm_rm); }
    }
    PMULHUW {
        execute: binary(|left, right| high_products(left, right, false));
        forms { P66 0x0f 0xe4 => operands(xmm, xmm_rm); }
    }
    PMULUDQ {
        execute: binary(unsigned_dword_products);
        forms { P66 0x0f 0xf4 => operands(xmm, xmm_rm); }
    }
    PMADDWD {
        execute: binary(|left, right| left.signed_dot_i16_pairs(right));
        forms { P66 0x0f 0xf5 => operands(xmm, xmm_rm); }
    }
}

fn high_products(left: Val<V128>, right: Val<V128>, signed: bool) -> Val<V128> {
    let (low, high) = if signed {
        (
            left.signed_widening_mul_low_lanes::<I16>(&right),
            left.signed_widening_mul_high_lanes::<I16>(right),
        )
    } else {
        (
            left.unsigned_widening_mul_low_lanes::<I16>(&right),
            left.unsigned_widening_mul_high_lanes::<I16>(right),
        )
    };
    // Logical shifts put the raw high words in the unsigned narrowing range.
    low.unsigned_shr_lanes::<I32>(16)
        .unsigned_narrow_lanes::<I16>(high.unsigned_shr_lanes::<I32>(16))
}

fn unsigned_dword_products(left: Val<V128>, right: Val<V128>) -> Val<V128> {
    // Gather dwords 0 and 2 before widening; the other input dwords are ignored.
    let indices = [0, 1, 2, 3, 8, 9, 10, 11, 0, 1, 2, 3, 8, 9, 10, 11];
    left.swizzle_bytes(indices)
        .unsigned_widening_mul_low_lanes::<I32>(right.swizzle_bytes(indices))
}
