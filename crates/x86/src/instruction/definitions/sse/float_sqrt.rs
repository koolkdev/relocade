//! SSE square root reads one source and preserves unused scalar destination lanes.

use super::{floating::square_root, *};
use crate::simd::FloatShape;

instruction_families! {
    SQRTSS {
        execute: square_root::<_>(FloatShape::Scalar);
        forms { PF3 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTSD {
        execute: square_root::<_>(FloatShape::Scalar);
        forms { PF2 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
    SQRTPS {
        execute: square_root::<_>(FloatShape::Packed);
        forms { NP 0x0f 0x51 => dword(xmm, xmm_rm); }
    }
    SQRTPD {
        execute: square_root::<_>(FloatShape::Packed);
        forms { P66 0x0f 0x51 => qword(xmm, xmm_rm); }
    }
}
