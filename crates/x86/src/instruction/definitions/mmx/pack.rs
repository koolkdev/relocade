//! MMX packing narrows the two logical 64-bit inputs with saturation.

use super::*;

instruction_families! {
    PACKSSWB {
        execute: binary::<_>(|left, right| left.signed_narrow_lanes::<I8>(right));
        forms { NP 0x0f 0x63 => qword(mm, mm_rm); }
    }
    PACKSSDW {
        execute: binary::<_>(|left, right| left.signed_narrow_lanes::<I16>(right));
        forms { NP 0x0f 0x6b => qword(mm, mm_rm); }
    }
    PACKUSWB {
        execute: binary::<_>(|left, right| left.unsigned_narrow_lanes::<I8>(right));
        forms { NP 0x0f 0x67 => qword(mm, mm_rm); }
    }
}
