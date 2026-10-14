//! SSE floating multiplication rounds the exact product to the destination format.

use super::{floating::arithmetic, *};
use crate::simd::{FloatArithmetic, FloatShape};

instruction_families! {
    MULSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Multiply);
        requires: [Sse];
        forms { PF3 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Multiply);
        requires: [Sse2];
        forms { PF2 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
    MULPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Multiply);
        requires: [Sse];
        forms { NP 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Multiply);
        requires: [Sse2];
        forms { P66 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
}
