//! Packed minima and maxima compare each lane at its signedness.

use super::*;

instruction_families! {
    PMINUB {
        execute: binary::<_>(|left, right| left.unsigned_min_lanes::<I8>(right));
        forms { NP 0x0f 0xda => qword(mm, mm_rm); }
    }
    PMAXUB {
        execute: binary::<_>(|left, right| left.unsigned_max_lanes::<I8>(right));
        forms { NP 0x0f 0xde => qword(mm, mm_rm); }
    }
    PMINSW {
        execute: binary::<_>(|left, right| left.signed_min_lanes::<I16>(right));
        forms { NP 0x0f 0xea => qword(mm, mm_rm); }
    }
    PMAXSW {
        execute: binary::<_>(|left, right| left.signed_max_lanes::<I16>(right));
        forms { NP 0x0f 0xee => qword(mm, mm_rm); }
    }
}
