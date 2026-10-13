//! Unsigned averages round upward without wrapping the intermediate sum.

use super::*;

instruction_families! {
    PAVGB {
        execute: binary::<_>(|left, right| left.unsigned_rounded_average_lanes::<I8>(right));
        forms { NP 0x0f 0xe0 => qword(mm, mm_rm); }
    }
    PAVGW {
        execute: binary::<_>(|left, right| left.unsigned_rounded_average_lanes::<I16>(right));
        forms { NP 0x0f 0xe3 => qword(mm, mm_rm); }
    }
}
