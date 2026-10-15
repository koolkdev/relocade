//! Packed minima and maxima compare each lane at its signedness.

use super::*;

instruction_families! {
    PMINUB {
        execute: binary(|left, right| left.unsigned_min_lanes::<I8>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xda => operands(xmm, xmm_rm); }
    }
    PMAXUB {
        execute: binary(|left, right| left.unsigned_max_lanes::<I8>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xde => operands(xmm, xmm_rm); }
    }
    PMINSW {
        execute: binary(|left, right| left.signed_min_lanes::<I16>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xea => operands(xmm, xmm_rm); }
    }
    PMAXSW {
        execute: binary(|left, right| left.signed_max_lanes::<I16>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xee => operands(xmm, xmm_rm); }
    }
}
