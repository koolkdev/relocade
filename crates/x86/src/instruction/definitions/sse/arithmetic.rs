//! Packed integer arithmetic wraps independently in all 128 bits.

use super::*;
use relocade_compiler::I64;

instruction_families! {
    PADDB {
        execute: binary(|left, right| left.add_lanes::<I8>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xfc => operands(xmm, xmm_rm); }
    }
    PADDW {
        execute: binary(|left, right| left.add_lanes::<I16>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xfd => operands(xmm, xmm_rm); }
    }
    PADDD {
        execute: binary(|left, right| left.add_lanes::<I32>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xfe => operands(xmm, xmm_rm); }
    }
    PADDQ {
        execute: binary(|left, right| left.add_lanes::<I64>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xd4 => operands(xmm, xmm_rm); }
    }
    PSUBB {
        execute: binary(|left, right| left.sub_lanes::<I8>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xf8 => operands(xmm, xmm_rm); }
    }
    PSUBW {
        execute: binary(|left, right| left.sub_lanes::<I16>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xf9 => operands(xmm, xmm_rm); }
    }
    PSUBD {
        execute: binary(|left, right| left.sub_lanes::<I32>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xfa => operands(xmm, xmm_rm); }
    }
    PSUBQ {
        execute: binary(|left, right| left.sub_lanes::<I64>(right));
        requires: [Sse2];
        forms { P66 0x0f 0xfb => operands(xmm, xmm_rm); }
    }
}
