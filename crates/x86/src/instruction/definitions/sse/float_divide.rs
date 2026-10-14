//! SSE floating division uses exact quotient evidence and MXCSR zero-divide reporting.

use super::{floating::arithmetic, *};
use crate::simd::{FloatArithmetic, FloatShape};

instruction_families! {
    DIVSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Divide);
        requires: [Sse];
        forms { PF3 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Divide);
        requires: [Sse2];
        forms { PF2 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
    DIVPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Divide);
        requires: [Sse];
        forms { NP 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Divide);
        requires: [Sse2];
        forms { P66 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
}
