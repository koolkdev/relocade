//! Signed dwords convert to scalar or packed floating encodings using MXCSR RC.

use super::*;
use crate::{
    floating::BinaryFormat,
    instruction::XmmType,
    simd::{integer_to_float, FloatLane, FloatResult},
};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    CVTSI2SS {
        execute: scalar(BinaryFormat::Binary32);
        forms { PF3 0x0f 0x2a => dword(xmm, rm); }
    }
    CVTSI2SD {
        execute: scalar(BinaryFormat::Binary64);
        forms { PF2 0x0f 0x2a => dword(xmm, rm); }
    }
    CVTDQ2PS {
        execute: packed::<_>;
        forms { NP 0x0f 0x5b => dword(xmm, xmm_rm); }
    }
    CVTDQ2PD {
        execute: packed::<_>;
        forms { PF3 0x0f 0xe6 => qword(xmm, xmm_rm); }
    }
}

fn scalar(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: Input<I32>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    let source = source.read(execution)?;
    match format {
        BinaryFormat::Binary32 => write_scalar::<I32>(execution, destination, source),
        BinaryFormat::Binary64 => write_scalar::<I64>(execution, destination, source),
    }
}

fn write_scalar<T: FloatLane + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: Val<I32>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.read_mxcsr()?;
    let result = integer_to_float::<T>(source, &controls.rounding());
    execution.record_simd_exceptions(&result.exceptions)?;
    T::write_register(execution, destination, result.value)
}

fn packed<T: FloatLane>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let source = match T::FORMAT {
        BinaryFormat::Binary32 => source.read::<V128>(execution, VectorAlignment::Aligned)?,
        BinaryFormat::Binary64 => {
            Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?)
        }
    };
    let controls = execution.read_mxcsr()?;
    let rounding = controls.rounding();
    let result = FloatResult::from_lanes(0_u128.into(), 16 / T::FORMAT.bytes(), |lane| {
        integer_to_float::<T>(source.extract_lane::<I32>(lane), &rounding)
    });
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}
