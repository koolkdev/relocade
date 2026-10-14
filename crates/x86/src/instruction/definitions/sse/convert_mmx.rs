//! MMX conversion bridges use MXCSR rounding and the shared x87 admission owner.

use super::{floating::IntegerRounding, *};
use crate::{
    floating::{BinaryEncoding, BinaryFormat},
    instruction::MmxLocation,
    simd::{integer_to_float, FloatOperand, FloatResult},
};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    CVTPI2PS {
        execute: from_integer::<_>;
        forms { NP 0x0f 0x2a => dword(xmm, mm_rm); }
    }
    CVTPI2PD {
        execute: from_integer::<_>;
        forms { P66 0x0f 0x2a => qword(xmm, mm_rm); }
    }
    CVTPS2PI {
        execute: to_integer::<_>(IntegerRounding::Mxcsr);
        forms { NP 0x0f 0x2d => dword(mm, xmm_rm); }
    }
    CVTPD2PI {
        execute: to_integer::<_>(IntegerRounding::Mxcsr);
        forms { P66 0x0f 0x2d => qword(mm, xmm_rm); }
    }
    CVTTPS2PI {
        execute: to_integer::<_>(IntegerRounding::Truncate);
        forms { NP 0x0f 0x2c => dword(mm, xmm_rm); }
    }
    CVTTPD2PI {
        execute: to_integer::<_>(IntegerRounding::Truncate);
        forms { P66 0x0f 0x2c => qword(mm, xmm_rm); }
    }
}

fn from_integer<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    // Intel exempts only CVTPI2PD's memory form from MMX admission and transition.
    let uses_mmx =
        T::FORMAT == BinaryFormat::Binary32 || matches!(&source, MmxLocation::Register(_));
    let convert = |execution: &mut ExecutionBuilder<'_, '_>| {
        let rounding = floating::rounding_for_integer_source(execution, T::FORMAT)?;
        let source = Val::<V128>::splat(source.read::<I64>(execution)?);
        let initial = match T::FORMAT {
            BinaryFormat::Binary32 => execution.read_xmm(destination.clone())?,
            BinaryFormat::Binary64 => 0_u128.into(),
        };
        let result = FloatResult::from_lanes(initial, 2, |lane| {
            integer_to_float::<T>(source.extract_lane::<I32>(lane), &rounding)
        });
        execution.record_simd_exceptions(&result.exceptions)?;
        execution.write_xmm(destination, result.value)
    };
    if uses_mmx {
        execution.execute_mmx(convert)
    } else {
        convert(execution)
    }
}

fn to_integer<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    rounding: IntegerRounding,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.execute_mmx(|execution| {
        let controls = execution.simd_controls()?;
        let rounding = rounding.mode(&controls);
        let source = match T::FORMAT {
            BinaryFormat::Binary32 => {
                Val::<V128>::splat(source.read::<I64>(execution, VectorAlignment::Unaligned)?)
            }
            BinaryFormat::Binary64 => source.read::<V128>(execution, VectorAlignment::Aligned)?,
        };
        let result = FloatResult::from_lanes(0_u128.into(), 2, |lane| {
            FloatOperand::new(
                source.extract_lane::<T>(lane),
                controls.denormals_are_zero(),
            )
            .to_integer(&rounding)
        });
        execution.record_simd_exceptions(&result.exceptions)?;
        execution.write_mmx(destination, result.value.low_half().to_bits())
    })
}
