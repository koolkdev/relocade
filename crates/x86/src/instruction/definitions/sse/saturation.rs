//! Signed and unsigned byte/word arithmetic clamps each lane to its own limits.

use super::*;

instruction_families! {
    PADDSB {
        execute: binary(|left, right| left.signed_saturating_add_lanes::<I8>(right));
        forms { P66 0x0f 0xec => operands(xmm, xmm_rm); }
    }
    PADDSW {
        execute: binary(|left, right| left.signed_saturating_add_lanes::<I16>(right));
        forms { P66 0x0f 0xed => operands(xmm, xmm_rm); }
    }
    PADDUSB {
        execute: binary(|left, right| left.unsigned_saturating_add_lanes::<I8>(right));
        forms { P66 0x0f 0xdc => operands(xmm, xmm_rm); }
    }
    PADDUSW {
        execute: binary(|left, right| left.unsigned_saturating_add_lanes::<I16>(right));
        forms { P66 0x0f 0xdd => operands(xmm, xmm_rm); }
    }
    PSUBSB {
        execute: binary(|left, right| left.signed_saturating_sub_lanes::<I8>(right));
        forms { P66 0x0f 0xe8 => operands(xmm, xmm_rm); }
    }
    PSUBSW {
        execute: binary(|left, right| left.signed_saturating_sub_lanes::<I16>(right));
        forms { P66 0x0f 0xe9 => operands(xmm, xmm_rm); }
    }
    PSUBUSB {
        execute: binary(|left, right| left.unsigned_saturating_sub_lanes::<I8>(right));
        forms { P66 0x0f 0xd8 => operands(xmm, xmm_rm); }
    }
    PSUBUSW {
        execute: binary(|left, right| left.unsigned_saturating_sub_lanes::<I16>(right));
        forms { P66 0x0f 0xd9 => operands(xmm, xmm_rm); }
    }
}
