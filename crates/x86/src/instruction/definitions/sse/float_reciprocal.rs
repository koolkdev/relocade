//! Reciprocal estimates use fixed rounding and preserve source NaN payloads.

use super::*;
use crate::{instruction::XmmType, simd::FloatEstimate};

instruction_families! {
    RCPSS {
        execute: scalar(FloatEstimate::Reciprocal);
        forms { PF3 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
    RCPPS {
        execute: packed(FloatEstimate::Reciprocal);
        forms { NP 0x0f 0x53 => operands(xmm, xmm_rm); }
    }
    RSQRTSS {
        execute: scalar(FloatEstimate::ReciprocalSquareRoot);
        forms { PF3 0x0f 0x52 => operands(xmm, xmm_rm); }
    }
    RSQRTPS {
        execute: packed(FloatEstimate::ReciprocalSquareRoot);
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
    let value = (0..4).fold(source.clone(), |result, lane| {
        result.replace_lane(
            lane,
            operation.calculate_scalar(source.extract_lane::<I32>(lane)),
        )
    });
    execution.write_xmm(destination, value)
}
