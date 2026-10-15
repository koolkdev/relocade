//! Scalar floating-to-integer conversions always write a signed dword GPR.

use super::floating::IntegerRounding;
use super::*;
use crate::{floating::BinaryFormat, simd::FloatOperand};
use wasm86_compiler::I64;

instruction_families! {
    CVTSS2SI {
        execute: convert(BinaryFormat::Binary32, IntegerRounding::Mxcsr);
        forms { PF3 0x0f 0x2d => dword(modrm_reg, xmm_rm); }
    }
    CVTSD2SI {
        execute: convert(BinaryFormat::Binary64, IntegerRounding::Mxcsr);
        forms { PF2 0x0f 0x2d => dword(modrm_reg, xmm_rm); }
    }
    CVTTSS2SI {
        execute: convert(BinaryFormat::Binary32, IntegerRounding::Truncate);
        forms { PF3 0x0f 0x2c => dword(modrm_reg, xmm_rm); }
    }
    CVTTSD2SI {
        execute: convert(BinaryFormat::Binary64, IntegerRounding::Truncate);
        forms { PF2 0x0f 0x2c => dword(modrm_reg, xmm_rm); }
    }
}

fn convert(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: XmmLocation,
    format: BinaryFormat,
    rounding: IntegerRounding,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    execution.specialize_simd_precision(&controls)?;
    let rounding = rounding.mode(&controls);
    let daz = controls.denormals_are_zero();
    let result = match format {
        BinaryFormat::Binary32 => FloatOperand::new(
            source.read::<I32>(execution, VectorAlignment::Unaligned)?,
            daz,
        )
        .to_integer(&rounding),
        BinaryFormat::Binary64 => FloatOperand::new(
            source.read::<I64>(execution, VectorAlignment::Unaligned)?,
            daz,
        )
        .to_integer(&rounding),
    };
    execution.record_simd_exceptions(&result.exceptions)?;
    destination.write(execution, result.value)
}
