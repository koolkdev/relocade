//! SSE addition and subtraction use MXCSR and preserve unused scalar lanes.

use super::{
    floating::{packed_arithmetic, scalar_arithmetic},
    *,
};
use crate::simd::FloatArithmetic;

instruction_families! {
    ADDSS {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Add);
        requires: [Sse];
        forms { PF3 0x0f 0x58 => dword(xmm, xmm_rm); }
    }
    ADDSD {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Add);
        requires: [Sse2];
        forms { PF2 0x0f 0x58 => qword(xmm, xmm_rm); }
    }
    ADDPS {
        execute: packed_arithmetic::<_>(FloatArithmetic::Add);
        requires: [Sse];
        forms { NP 0x0f 0x58 => dword(xmm, xmm_rm); }
    }
    ADDPD {
        execute: packed_arithmetic::<_>(FloatArithmetic::Add);
        requires: [Sse2];
        forms { P66 0x0f 0x58 => qword(xmm, xmm_rm); }
    }
    SUBSS {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Subtract);
        requires: [Sse];
        forms { PF3 0x0f 0x5c => dword(xmm, xmm_rm); }
    }
    SUBSD {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Subtract);
        requires: [Sse2];
        forms { PF2 0x0f 0x5c => qword(xmm, xmm_rm); }
    }
    SUBPS {
        execute: packed_arithmetic::<_>(FloatArithmetic::Subtract);
        requires: [Sse];
        forms { NP 0x0f 0x5c => dword(xmm, xmm_rm); }
    }
    SUBPD {
        execute: packed_arithmetic::<_>(FloatArithmetic::Subtract);
        requires: [Sse2];
        forms { P66 0x0f 0x5c => qword(xmm, xmm_rm); }
    }
}
