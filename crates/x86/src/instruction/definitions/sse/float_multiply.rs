//! SSE floating multiplication rounds the exact product to the destination format.

use super::{floating::arithmetic, *};
use crate::simd::{FloatArithmetic, FloatShape};

instruction_families! {
    MULSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Multiply);
        forms { PF3 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Multiply);
        forms { PF2 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
    MULPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Multiply);
        forms { NP 0x0f 0x59 => dword(xmm, xmm_rm); }
    }
    MULPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Multiply);
        forms { P66 0x0f 0x59 => qword(xmm, xmm_rm); }
    }
}
