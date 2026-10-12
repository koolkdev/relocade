//! MMX unpacking interleaves one 32-bit half from each input.
//! Low forms read m32 sources; high forms read m64 sources.

use super::*;
use crate::alu::unpack_lanes;

instruction_families! {
    PUNPCKLBW {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 1, 0));
        forms { NP 0x0f 0x60 => dword(mm, mm_rm); }
    }
    PUNPCKLWD {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 2, 0));
        forms { NP 0x0f 0x61 => dword(mm, mm_rm); }
    }
    PUNPCKLDQ {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 4, 0));
        forms { NP 0x0f 0x62 => dword(mm, mm_rm); }
    }
    PUNPCKHBW {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 1, 4));
        forms { NP 0x0f 0x68 => qword(mm, mm_rm); }
    }
    PUNPCKHWD {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 2, 4));
        forms { NP 0x0f 0x69 => qword(mm, mm_rm); }
    }
    PUNPCKHDQ {
        execute: binary::<_>(|left, right| unpack_lanes(left, right, 4, 4));
        forms { NP 0x0f 0x6a => qword(mm, mm_rm); }
    }
}
