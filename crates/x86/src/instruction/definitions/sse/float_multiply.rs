//! SSE floating multiplication rounds the exact product to the destination format.

use super::{
    floating::{packed_arithmetic, scalar_arithmetic},
    *,
};
use crate::simd::FloatArithmetic;

instruction_families! {
    MULSS {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Multiply);
        requires: [Sse];
        forms { PF3 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULSD {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Multiply);
        requires: [Sse2];
        forms { PF2 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
    MULPS {
        execute: packed_arithmetic::<_>(FloatArithmetic::Multiply);
        requires: [Sse];
        forms { NP 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULPD {
        execute: packed_arithmetic::<_>(FloatArithmetic::Multiply);
        requires: [Sse2];
        forms { P66 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
}
