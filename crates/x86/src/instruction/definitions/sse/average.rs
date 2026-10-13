//! Unsigned averages round upward without wrapping the intermediate sum.

use super::*;

instruction_families! {
    PAVGB {
        execute: binary(|left, right| left.unsigned_rounded_average_lanes::<I8>(right));
        forms { P66 0x0f 0xe0 => operands(xmm, xmm_rm); }
    }
    PAVGW {
        execute: binary(|left, right| left.unsigned_rounded_average_lanes::<I16>(right));
        forms { P66 0x0f 0xe3 => operands(xmm, xmm_rm); }
    }
}
