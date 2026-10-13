//! Legacy floating-point minima and maxima select the source on ties or NaNs.

use super::{floating::binary as float_binary, *};
use crate::simd::{FloatLane, FloatOperands, FloatResult, FloatShape};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    MINSS {
        execute: float_binary::<_>(FloatShape::Scalar, |operands| min_max(operands, MinMax::Min));
        forms { PF3 0x0f 0x5d => dword(xmm, xmm_rm); }
    }
    MINSD {
        execute: float_binary::<_>(FloatShape::Scalar, |operands| min_max(operands, MinMax::Min));
        forms { PF2 0x0f 0x5d => qword(xmm, xmm_rm); }
    }
    MINPS {
        execute: float_binary::<_>(FloatShape::Packed, |operands| min_max(operands, MinMax::Min));
        forms { NP 0x0f 0x5d => dword(xmm, xmm_rm); }
    }
    MINPD {
        execute: float_binary::<_>(FloatShape::Packed, |operands| min_max(operands, MinMax::Min));
        forms { P66 0x0f 0x5d => qword(xmm, xmm_rm); }
    }
    MAXSS {
        execute: float_binary::<_>(FloatShape::Scalar, |operands| min_max(operands, MinMax::Max));
        forms { PF3 0x0f 0x5f => dword(xmm, xmm_rm); }
    }
    MAXSD {
        execute: float_binary::<_>(FloatShape::Scalar, |operands| min_max(operands, MinMax::Max));
        forms { PF2 0x0f 0x5f => qword(xmm, xmm_rm); }
    }
    MAXPS {
        execute: float_binary::<_>(FloatShape::Packed, |operands| min_max(operands, MinMax::Max));
        forms { NP 0x0f 0x5f => dword(xmm, xmm_rm); }
    }
    MAXPD {
        execute: float_binary::<_>(FloatShape::Packed, |operands| min_max(operands, MinMax::Max));
        forms { P66 0x0f 0x5f => qword(xmm, xmm_rm); }
    }
}

enum MinMax {
    Min,
    Max,
}

fn min_max<T: FloatLane>(operands: FloatOperands<T>, operation: MinMax) -> FloatResult<T>
where
    I64: AtLeast<T>,
{
    let comparison = operands.compare(true.into());
    let take_left = match operation {
        MinMax::Min => comparison.less,
        MinMax::Max => comparison.greater,
    };
    FloatResult {
        // Ties (including opposite zero signs) and NaNs select the source.
        // A selected SNaN retains its signaling bit and payload.
        value: take_left.select(operands.left, operands.right),
        exceptions: comparison.exceptions,
    }
}
