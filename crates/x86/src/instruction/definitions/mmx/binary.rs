//! MMX logical operations and packed arithmetic.

use super::*;

instruction_families! {
    PCMPEQB {
        execute: packed_binary(|left, right| left.eq_lanes::<I8>(right));
        forms { NP 0x0f 0x74 => operands(mm, mm_rm); }
    }
    PCMPEQW {
        execute: packed_binary(|left, right| left.eq_lanes::<I16>(right));
        forms { NP 0x0f 0x75 => operands(mm, mm_rm); }
    }
    PCMPEQD {
        execute: packed_binary(|left, right| left.eq_lanes::<I32>(right));
        forms { NP 0x0f 0x76 => operands(mm, mm_rm); }
    }
    PCMPGTB {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I8>(right));
        forms { NP 0x0f 0x64 => operands(mm, mm_rm); }
    }
    PCMPGTW {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I16>(right));
        forms { NP 0x0f 0x65 => operands(mm, mm_rm); }
    }
    PCMPGTD {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I32>(right));
        forms { NP 0x0f 0x66 => operands(mm, mm_rm); }
    }

    PADDB {
        execute: packed_binary(|left, right| left.add_lanes::<I8>(right));
        forms { NP 0x0f 0xfc => operands(mm, mm_rm); }
    }
    PADDW {
        execute: packed_binary(|left, right| left.add_lanes::<I16>(right));
        forms { NP 0x0f 0xfd => operands(mm, mm_rm); }
    }
    PADDD {
        execute: packed_binary(|left, right| left.add_lanes::<I32>(right));
        forms { NP 0x0f 0xfe => operands(mm, mm_rm); }
    }
    PSUBB {
        execute: packed_binary(|left, right| left.sub_lanes::<I8>(right));
        forms { NP 0x0f 0xf8 => operands(mm, mm_rm); }
    }
    PSUBW {
        execute: packed_binary(|left, right| left.sub_lanes::<I16>(right));
        forms { NP 0x0f 0xf9 => operands(mm, mm_rm); }
    }
    PSUBD {
        execute: packed_binary(|left, right| left.sub_lanes::<I32>(right));
        forms { NP 0x0f 0xfa => operands(mm, mm_rm); }
    }

    PAND {
        execute: binary(|left, right| left.and(right));
        forms { NP 0x0f 0xdb => operands(mm, mm_rm); }
    }
    PANDN {
        execute: binary(|left, right| left.xor(u64::MAX).and(right));
        forms { NP 0x0f 0xdf => operands(mm, mm_rm); }
    }
    POR {
        execute: binary(|left, right| left.or(right));
        forms { NP 0x0f 0xeb => operands(mm, mm_rm); }
    }
    PXOR {
        execute: binary(|left, right| left.xor(right));
        forms { NP 0x0f 0xef => operands(mm, mm_rm); }
    }
}
