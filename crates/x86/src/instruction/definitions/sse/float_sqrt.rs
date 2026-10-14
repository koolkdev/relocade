//! SSE square root reads one source and preserves unused scalar destination lanes.

use super::{floating::square_root, *};
use crate::simd::FloatShape;

instruction_families! {
    SQRTSS {
        execute: square_root::<_>(FloatShape::Scalar);
        requires: [Sse];
        forms { PF3 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTSD {
        execute: square_root::<_>(FloatShape::Scalar);
        requires: [Sse2];
        forms { PF2 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
    SQRTPS {
        execute: square_root::<_>(FloatShape::Packed);
        requires: [Sse];
        forms { NP 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTPD {
        execute: square_root::<_>(FloatShape::Packed);
        requires: [Sse2];
        forms { P66 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
}
