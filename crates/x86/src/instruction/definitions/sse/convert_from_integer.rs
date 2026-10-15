//! Signed dwords convert to scalar or packed floating encodings using MXCSR RC.

use super::*;
use crate::{
    floating::{BinaryEncoding, BinaryFormat, RoundingMode},
    instruction::XmmType,
    simd::{integer_to_float, packed_integer_to_float},
};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    CVTSI2SS {
        execute: scalar(BinaryFormat::Binary32);
        requires: [Sse];
        forms { PF3 0x0f 0x2a => dword(xmm, rm); }
    }
    CVTSI2SD {
        execute: scalar(BinaryFormat::Binary64);
        requires: [Sse2];
        forms { PF2 0x0f 0x2a => dword(xmm, rm); }
    }
    CVTDQ2PS {
        execute: packed::<_>;
        requires: [Sse2];
        forms { NP 0x0f 0x5b => dword(xmm, xmm_rm); }
    }
    CVTDQ2PD {
        execute: packed::<_>;
        requires: [Sse2];
        forms { PF3 0x0f 0xe6 => qword(xmm, xmm_rm); }
    }
}

fn scalar(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: Input<I32>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    let rounding = floating::rounding_for_integer_source(execution, format)?;
    let source = source.read(execution)?;
    match format {
        BinaryFormat::Binary32 => write_scalar::<I32>(execution, destination, source, &rounding),
        BinaryFormat::Binary64 => write_scalar::<I64>(execution, destination, source, &rounding),
    }
}

fn write_scalar<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: Val<I32>,
    rounding: &RoundingMode,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let result = integer_to_float::<T>(source, rounding);
    execution.record_simd_exceptions(&result.exceptions)?;
    T::write_register(execution, destination, result.value)
}

fn packed<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let rounding = floating::rounding_for_integer_source(execution, T::FORMAT)?;
    let source = match T::FORMAT {
        BinaryFormat::Binary32 => source.read::<V128>(execution, VectorAlignment::Aligned)?,
        BinaryFormat::Binary64 => {
            Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?)
        }
    };
    let result = packed_integer_to_float(T::FORMAT, source, &rounding);
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}
