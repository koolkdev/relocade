//! Reciprocal estimates use fixed rounding and preserve source NaN payloads.

use super::{floating::approximate, *};
use crate::simd::{FloatOperand, FloatShape};
use wasm86_compiler::F64;

instruction_families! {
    RCPSS {
        execute: approximate(FloatShape::Scalar, reciprocal);
        forms { PF3 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
    RCPPS {
        execute: approximate(FloatShape::Packed, reciprocal);
        forms { NP 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
}

fn reciprocal(bits: Val<I32>) -> Val<I32> {
    // Both signs of denormal input become zero regardless of MXCSR.DAZ.
    let operand = FloatOperand::new(bits, true.into());
    // Native division and binary32 rounding are more accurate than the required
    // relative error bound of 1.5 * 2^-12; no CPU-specific estimate table is needed.
    let result = Val::<F64>::from(1.0)
        .div(operand.value())
        .to_binary32_bits();
    // RCP always flushes tiny outputs, independently of MXCSR.FTZ.
    let result = result
        .and(0x7f80_0000)
        .eq(0)
        .select(result.and(0x8000_0000_u32), result);
    operand
        .classification()
        .nan()
        .select(operand.bits.or(0x0040_0000), result)
}
