//! PSADBW returns independent byte sums in the low word of each XMM qword.

use super::*;
use crate::simd::sum_absolute_byte_differences;

instruction_families! {
    PSADBW {
        execute: binary(sum_absolute_byte_differences);
        requires: [Sse2];
        forms { P66 0x0f 0xf6 => operands(xmm, xmm_rm); }
    }
}
