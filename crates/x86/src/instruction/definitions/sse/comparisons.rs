//! Packed comparisons produce a full-width mask in each byte, word or dword lane.

use super::*;

instruction_families! {
    PCMPEQB {
        execute: binary(|left, right| left.eq_lanes::<I8>(right));
        forms { P66 0x0f 0x74 => operands(xmm, xmm_rm); }
    }
    PCMPEQW {
        execute: binary(|left, right| left.eq_lanes::<I16>(right));
        forms { P66 0x0f 0x75 => operands(xmm, xmm_rm); }
    }
    PCMPEQD {
        execute: binary(|left, right| left.eq_lanes::<I32>(right));
        forms { P66 0x0f 0x76 => operands(xmm, xmm_rm); }
    }
    PCMPGTB {
        execute: binary(|left, right| left.signed_gt_lanes::<I8>(right));
        forms { P66 0x0f 0x64 => operands(xmm, xmm_rm); }
    }
    PCMPGTW {
        execute: binary(|left, right| left.signed_gt_lanes::<I16>(right));
        forms { P66 0x0f 0x65 => operands(xmm, xmm_rm); }
    }
    PCMPGTD {
        execute: binary(|left, right| left.signed_gt_lanes::<I32>(right));
        forms { P66 0x0f 0x66 => operands(xmm, xmm_rm); }
    }
}
