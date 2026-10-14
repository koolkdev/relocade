//! SSE floating division uses exact quotient evidence and MXCSR zero-divide reporting.

use super::{floating::arithmetic, *};
use crate::simd::FloatArithmetic;

instruction_families! {
    DIVSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Divide);
        forms { PF3 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Divide);
        forms { PF2 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
    DIVPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Divide);
        forms { NP 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Divide);
        forms { P66 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
}
