//! MMX logical operations and packed arithmetic.

use super::*;

instruction_families! {
    PADDSB {
        execute: binary::<_>(|left, right| left.signed_saturating_add_lanes::<I8>(right));
        forms { NP 0x0f 0xec => qword(mm, mm_rm); }
    }
    PADDSW {
        execute: binary::<_>(|left, right| left.signed_saturating_add_lanes::<I16>(right));
        forms { NP 0x0f 0xed => qword(mm, mm_rm); }
    }
    PADDUSB {
        execute: binary::<_>(|left, right| left.unsigned_saturating_add_lanes::<I8>(right));
        forms { NP 0x0f 0xdc => qword(mm, mm_rm); }
    }
    PADDUSW {
        execute: binary::<_>(|left, right| left.unsigned_saturating_add_lanes::<I16>(right));
        forms { NP 0x0f 0xdd => qword(mm, mm_rm); }
    }
    PSUBSB {
        execute: binary::<_>(|left, right| left.signed_saturating_sub_lanes::<I8>(right));
        forms { NP 0x0f 0xe8 => qword(mm, mm_rm); }
    }
    PSUBSW {
        execute: binary::<_>(|left, right| left.signed_saturating_sub_lanes::<I16>(right));
        forms { NP 0x0f 0xe9 => qword(mm, mm_rm); }
    }
    PSUBUSB {
        execute: binary::<_>(|left, right| left.unsigned_saturating_sub_lanes::<I8>(right));
        forms { NP 0x0f 0xd8 => qword(mm, mm_rm); }
    }
    PSUBUSW {
        execute: binary::<_>(|left, right| left.unsigned_saturating_sub_lanes::<I16>(right));
        forms { NP 0x0f 0xd9 => qword(mm, mm_rm); }
    }

    PCMPEQB {
        execute: binary::<_>(|left, right| left.eq_lanes::<I8>(right));
        forms { NP 0x0f 0x74 => qword(mm, mm_rm); }
    }
    PCMPEQW {
        execute: binary::<_>(|left, right| left.eq_lanes::<I16>(right));
        forms { NP 0x0f 0x75 => qword(mm, mm_rm); }
    }
    PCMPEQD {
        execute: binary::<_>(|left, right| left.eq_lanes::<I32>(right));
        forms { NP 0x0f 0x76 => qword(mm, mm_rm); }
    }
    PCMPGTB {
        execute: binary::<_>(|left, right| left.signed_gt_lanes::<I8>(right));
        forms { NP 0x0f 0x64 => qword(mm, mm_rm); }
    }
    PCMPGTW {
        execute: binary::<_>(|left, right| left.signed_gt_lanes::<I16>(right));
        forms { NP 0x0f 0x65 => qword(mm, mm_rm); }
    }
    PCMPGTD {
        execute: binary::<_>(|left, right| left.signed_gt_lanes::<I32>(right));
        forms { NP 0x0f 0x66 => qword(mm, mm_rm); }
    }

    PADDB {
        execute: binary::<_>(|left, right| left.add_lanes::<I8>(right));
        forms { NP 0x0f 0xfc => qword(mm, mm_rm); }
    }
    PADDW {
        execute: binary::<_>(|left, right| left.add_lanes::<I16>(right));
        forms { NP 0x0f 0xfd => qword(mm, mm_rm); }
    }
    PADDD {
        execute: binary::<_>(|left, right| left.add_lanes::<I32>(right));
        forms { NP 0x0f 0xfe => qword(mm, mm_rm); }
    }
    PADDQ {
        execute: binary::<_>(|left, right| left.add_lanes::<I64>(right));
        forms { NP 0x0f 0xd4 => qword(mm, mm_rm); }
    }
    PSUBB {
        execute: binary::<_>(|left, right| left.sub_lanes::<I8>(right));
        forms { NP 0x0f 0xf8 => qword(mm, mm_rm); }
    }
    PSUBW {
        execute: binary::<_>(|left, right| left.sub_lanes::<I16>(right));
        forms { NP 0x0f 0xf9 => qword(mm, mm_rm); }
    }
    PSUBD {
        execute: binary::<_>(|left, right| left.sub_lanes::<I32>(right));
        forms { NP 0x0f 0xfa => qword(mm, mm_rm); }
    }
    PSUBQ {
        execute: binary::<_>(|left, right| left.sub_lanes::<I64>(right));
        forms { NP 0x0f 0xfb => qword(mm, mm_rm); }
    }

    PAND {
        execute: binary::<_>(|left, right| left.and(right));
        forms { NP 0x0f 0xdb => qword(mm, mm_rm); }
    }
    PANDN {
        execute: binary::<_>(|left, right| left.xor(u64::MAX).and(right));
        forms { NP 0x0f 0xdf => qword(mm, mm_rm); }
    }
    POR {
        execute: binary::<_>(|left, right| left.or(right));
        forms { NP 0x0f 0xeb => qword(mm, mm_rm); }
    }
    PXOR {
        execute: binary::<_>(|left, right| left.xor(right));
        forms { NP 0x0f 0xef => qword(mm, mm_rm); }
    }
}
