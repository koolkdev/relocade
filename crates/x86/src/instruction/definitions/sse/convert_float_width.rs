//! Scalar width conversions preserve upper bits; packed conversions replace XMM.

use super::*;
use crate::{
    instruction::XmmType,
    simd::{narrow_packed_float, widen_packed_float, FloatOperand, FloatResult},
};
use wasm86_compiler::I64;

instruction_families! {
    CVTSS2SD {
        execute: widen_scalar;
        requires: [Sse2];
        forms { PF3 0x0f 0x5a => dword(xmm, xmm_rm); }
    }
    CVTSD2SS {
        execute: narrow_scalar;
        requires: [Sse2];
        forms { PF2 0x0f 0x5a => qword(xmm, xmm_rm); }
    }
    CVTPS2PD {
        execute: widen_packed;
        requires: [Sse2];
        forms { NP 0x0f 0x5a => dword(xmm, xmm_rm); }
    }
    CVTPD2PS {
        execute: narrow_packed;
        requires: [Sse2];
        forms { P66 0x0f 0x5a => qword(xmm, xmm_rm); }
    }
}

fn widen_scalar(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    let source = source.read::<I32>(execution, VectorAlignment::Unaligned)?;
    let result = FloatOperand::new(source, controls.denormals_are_zero()).widen();
    execution.record_simd_exceptions(&result.exceptions)?;
    I64::write_register(execution, destination, result.value)
}

fn narrow_scalar(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    let source = source.read::<I64>(execution, VectorAlignment::Unaligned)?;
    let guarded_result = execution.if_can_fallback(|execution| {
        // Only the active source participates in admission and precision reporting.
        let candidate = narrow_packed_float(
            Val::<V128>::splat(source.clone()),
            controls.denormals_are_zero(),
            &controls.rounding(),
        );
        let result = execution.admit_simd_candidate(candidate)?;
        Ok(FloatResult {
            value: result.value.extract_lane::<I32>(0),
            exceptions: result.exceptions,
        })
    })?;
    let result = guarded_result.unwrap_or_else(|| {
        FloatOperand::new(source, controls.denormals_are_zero()).narrow(&controls)
    });
    execution.record_simd_exceptions(&result.exceptions)?;
    I32::write_register(execution, destination, result.value)
}

fn widen_packed(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    let source = Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?);
    let guarded_result = execution.if_can_fallback(|execution| {
        let candidate = widen_packed_float(source.clone(), controls.denormals_are_zero());
        execution.admit_simd_candidate(candidate)
    })?;
    let result = match guarded_result {
        Some(result) => result,
        None => FloatResult::from_lanes(0_u128.into(), 2, |lane| {
            FloatOperand::new(
                source.extract_lane::<I32>(lane),
                controls.denormals_are_zero(),
            )
            .widen()
        }),
    };
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}

fn narrow_packed(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    let source = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let guarded_result = execution.if_can_fallback(|execution| {
        let candidate = narrow_packed_float(
            source.clone(),
            controls.denormals_are_zero(),
            &controls.rounding(),
        );
        execution.admit_simd_candidate(candidate)
    })?;
    let result = match guarded_result {
        Some(result) => result,
        None => FloatResult::from_lanes(0_u128.into(), 2, |lane| {
            FloatOperand::new(
                source.extract_lane::<I64>(lane),
                controls.denormals_are_zero(),
            )
            .narrow(&controls)
        }),
    };
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}
