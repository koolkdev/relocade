//! XMM unpacking interleaves one 64-bit half from each input.

use super::*;
use crate::simd::unpack_lanes;

instruction_families! {
    UNPCKLPS {
        execute: binary(|left, right| unpack_lanes(left, right, 4, 0));
        forms { NP 0x0f 0x14 => operands(xmm, xmm_rm); }
    }
    UNPCKLPD {
        execute: binary(|left, right| unpack_lanes(left, right, 8, 0));
        forms { P66 0x0f 0x14 => operands(xmm, xmm_rm); }
    }
    UNPCKHPS {
        execute: binary(|left, right| unpack_lanes(left, right, 4, 8));
        forms { NP 0x0f 0x15 => operands(xmm, xmm_rm); }
    }
    UNPCKHPD {
        execute: binary(|left, right| unpack_lanes(left, right, 8, 8));
        forms { P66 0x0f 0x15 => operands(xmm, xmm_rm); }
    }
    PUNPCKLBW {
        execute: binary(|left, right| unpack_lanes(left, right, 1, 0));
        forms { P66 0x0f 0x60 => operands(xmm, xmm_rm); }
    }
    PUNPCKLWD {
        execute: binary(|left, right| unpack_lanes(left, right, 2, 0));
        forms { P66 0x0f 0x61 => operands(xmm, xmm_rm); }
    }
    PUNPCKLDQ {
        execute: binary(|left, right| unpack_lanes(left, right, 4, 0));
        forms { P66 0x0f 0x62 => operands(xmm, xmm_rm); }
    }
    PUNPCKLQDQ {
        execute: binary(|left, right| unpack_lanes(left, right, 8, 0));
        forms { P66 0x0f 0x6c => operands(xmm, xmm_rm); }
    }
    PUNPCKHBW {
        execute: binary(|left, right| unpack_lanes(left, right, 1, 8));
        forms { P66 0x0f 0x68 => operands(xmm, xmm_rm); }
    }
    PUNPCKHWD {
        execute: binary(|left, right| unpack_lanes(left, right, 2, 8));
        forms { P66 0x0f 0x69 => operands(xmm, xmm_rm); }
    }
    PUNPCKHDQ {
        execute: binary(|left, right| unpack_lanes(left, right, 4, 8));
        forms { P66 0x0f 0x6a => operands(xmm, xmm_rm); }
    }
    PUNPCKHQDQ {
        execute: binary(|left, right| unpack_lanes(left, right, 8, 8));
        forms { P66 0x0f 0x6d => operands(xmm, xmm_rm); }
    }
}
