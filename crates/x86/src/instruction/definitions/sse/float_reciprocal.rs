//! Reciprocal estimates use fixed rounding and preserve source NaN payloads.

use super::*;
use crate::{
    instruction::XmmType,
    simd::{estimate_packed_float, FloatEstimate},
};

instruction_families! {
    RCPSS {
        execute: scalar(FloatEstimate::Reciprocal);
        requires: [Sse];
        forms { PF3 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
    RCPPS {
        execute: packed(FloatEstimate::Reciprocal);
        requires: [Sse];
        forms { NP 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
    RSQRTSS {
        execute: scalar(FloatEstimate::ReciprocalSquareRoot);
        requires: [Sse];
        forms { PF3 0x0f 0x52 => operands(xmm, xmm_rm); }
    }
    RSQRTPS {
        execute: packed(FloatEstimate::ReciprocalSquareRoot);
        requires: [Sse];
        forms { NP 0x0f 0x52 => operands(xmm, xmm_rm); }
    }
}

fn scalar(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatEstimate,
) -> Result<(), BuildError> {
    let source = source.read::<I32>(execution, VectorAlignment::Unaligned)?;
    I32::write_register(execution, destination, operation.calculate_scalar(source))
}

fn packed(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatEstimate,
) -> Result<(), BuildError> {
    let source = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    execution.write_xmm(destination, estimate_packed_float(source, operation))
}
