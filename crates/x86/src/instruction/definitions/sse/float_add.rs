//! SSE addition and subtraction use MXCSR and preserve unused scalar lanes.

use super::{floating::arithmetic, *};
use crate::simd::{FloatArithmetic, FloatShape};

instruction_families! {
    ADDSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Add);
        requires: [Sse];
        forms { PF3 0x0f 0x58 => dword(xmm, xmm_rm); }
    }
    ADDSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Add);
        requires: [Sse2];
        forms { PF2 0x0f 0x58 => qword(xmm, xmm_rm); }
    }
    ADDPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Add);
        requires: [Sse];
        forms { NP 0x0f 0x58 => dword(xmm, xmm_rm); }
    }
    ADDPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Add);
        requires: [Sse2];
        forms { P66 0x0f 0x58 => qword(xmm, xmm_rm); }
    }
    SUBSS {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Subtract);
        requires: [Sse];
        forms { PF3 0x0f 0x5c => dword(xmm, xmm_rm); }
    }
    SUBSD {
        execute: arithmetic::<_>(FloatShape::Scalar, FloatArithmetic::Subtract);
        requires: [Sse2];
        forms { PF2 0x0f 0x5c => qword(xmm, xmm_rm); }
    }
    SUBPS {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Subtract);
        requires: [Sse];
        forms { NP 0x0f 0x5c => dword(xmm, xmm_rm); }
    }
    SUBPD {
        execute: arithmetic::<_>(FloatShape::Packed, FloatArithmetic::Subtract);
        requires: [Sse2];
        forms { P66 0x0f 0x5c => qword(xmm, xmm_rm); }
    }
}
