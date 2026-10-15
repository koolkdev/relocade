//! SSE square root reads one source and preserves unused scalar destination lanes.

use super::{
    floating::{packed_square_root, scalar_square_root},
    *,
};

instruction_families! {
    SQRTSS {
        execute: scalar_square_root::<_>();
        requires: [Sse];
        forms { PF3 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTSD {
        execute: scalar_square_root::<_>();
        requires: [Sse2];
        forms { PF2 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
    SQRTPS {
        execute: packed_square_root::<_>();
        requires: [Sse];
        forms { NP 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTPD {
        execute: packed_square_root::<_>();
        requires: [Sse2];
        forms { P66 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
}
