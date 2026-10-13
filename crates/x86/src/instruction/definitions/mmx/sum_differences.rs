//! PSADBW returns one eight-byte sum in the low word of the MMX destination.

use super::*;
use crate::simd::sum_absolute_byte_differences;

instruction_families! {
    PSADBW {
        execute: binary::<_>(sum_absolute_byte_differences);
        forms { NP 0x0f 0xf6 => qword(mm, mm_rm); }
    }
}
