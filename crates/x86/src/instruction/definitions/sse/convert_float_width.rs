//! Scalar width conversions preserve upper bits; packed conversions replace XMM.

use super::*;
use crate::simd::{FloatOperand, FloatResult, FloatShape};
use wasm86_compiler::I64;

instruction_families! {
    CVTSS2SD {
        execute: widen(FloatShape::Scalar);
        forms { PF3 0x0f 0x5a => dword(xmm, xmm_rm); }
    }
    CVTSD2SS {
        execute: narrow(FloatShape::Scalar);
        forms { PF2 0x0f 0x5a => qword(xmm, xmm_rm); }
    }
    CVTPS2PD {
        execute: widen(FloatShape::Packed);
        forms { NP 0x0f 0x5a => dword(xmm, xmm_rm); }
    }
    CVTPD2PS {
        execute: narrow(FloatShape::Packed);
        forms { P66 0x0f 0x5a => qword(xmm, xmm_rm); }
    }
}

fn widen(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
) -> Result<(), BuildError> {
    let (source, initial, count) = match shape {
        FloatShape::Scalar => (
            Val::<V128>::splat(source.read::<I32>(execution, VectorAlignment::Unaligned)?),
            execution.read_xmm(destination.clone())?,
            1,
        ),
        FloatShape::Packed => (
            Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?),
            0_u128.into(),
            2,
        ),
    };
    let controls = execution.read_mxcsr()?;
    let result = FloatResult::from_lanes(initial, count, |lane| {
        FloatOperand::new(
            source.extract_lane::<I32>(lane),
            controls.denormals_are_zero(),
        )
        .widen()
    });
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}

fn narrow(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
) -> Result<(), BuildError> {
    let (source, initial, count) = match shape {
        FloatShape::Scalar => (
            Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?),
            execution.read_xmm(destination.clone())?,
            1,
        ),
        FloatShape::Packed => (
            source.read::<V128>(execution, VectorAlignment::Aligned)?,
            0_u128.into(),
            2,
        ),
    };
    let controls = execution.read_mxcsr()?;
    let result = FloatResult::from_lanes(initial, count, |lane| {
        FloatOperand::new(
            source.extract_lane::<I64>(lane),
            controls.denormals_are_zero(),
        )
        .narrow(&controls)
    });
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}
