//! SSE floating division uses exact quotient evidence and MXCSR zero-divide reporting.

use super::{
    floating::{packed_arithmetic, scalar_arithmetic},
    *,
};
use crate::simd::FloatArithmetic;

instruction_families! {
    DIVSS {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Divide);
        forms { PF3 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVSD {
        execute: scalar_arithmetic::<_>(FloatArithmetic::Divide);
        forms { PF2 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
    DIVPS {
        execute: packed_arithmetic::<_>(FloatArithmetic::Divide);
        forms { NP 0x0f 0x5e => dword(xmm, xmm_rm); }
    }
    DIVPD {
        execute: packed_arithmetic::<_>(FloatArithmetic::Divide);
        forms { P66 0x0f 0x5e => qword(xmm, xmm_rm); }
    }
}
