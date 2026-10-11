//! Packing narrows both complete sources, with the destination's lanes first.

use super::*;

instruction_families! {
    PACKSSWB {
        execute: binary(|left, right| left.signed_narrow_lanes::<I8>(right));
        forms { P66 0x0f 0x63 => operands(xmm, xmm_rm); }
    }
    PACKSSDW {
        execute: binary(|left, right| left.signed_narrow_lanes::<I16>(right));
        forms { P66 0x0f 0x6b => operands(xmm, xmm_rm); }
    }
    PACKUSWB {
        // Inputs are signed words even though the saturated output bytes are unsigned.
        execute: binary(|left, right| left.unsigned_narrow_lanes::<I8>(right));
        forms { P66 0x0f 0x67 => operands(xmm, xmm_rm); }
    }
}
