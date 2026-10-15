//! Floating comparisons publish MXCSR before changing EFLAGS or XMM masks.

use super::*;
use crate::{
    flags::{Flag, FlagChange},
    floating::BinaryEncoding,
    instruction::XmmType,
    simd::{FloatOperands, PackedFloatOperand},
};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    COMISS {
        execute: compare_flags::<_>(true);
        forms { NP 0x0f 0x2f => dword(xmm, xmm_rm); }
    }
    UCOMISS {
        execute: compare_flags::<_>(false);
        forms { NP 0x0f 0x2e => dword(xmm, xmm_rm); }
    }
    COMISD {
        execute: compare_flags::<_>(true);
        forms { P66 0x0f 0x2f => qword(xmm, xmm_rm); }
    }
    UCOMISD {
        execute: compare_flags::<_>(false);
        forms { P66 0x0f 0x2e => qword(xmm, xmm_rm); }
    }
    CMPSS {
        execute: compare_scalar_mask::<_>;
        forms { PF3 0x0f 0xc2 => dword(xmm, xmm_rm, imm8); }
    }
    CMPSD {
        execute: compare_scalar_mask::<_>;
        forms { PF2 0x0f 0xc2 => qword(xmm, xmm_rm, imm8); }
    }
    CMPPS {
        execute: compare_packed_mask::<_>;
        forms { NP 0x0f 0xc2 => dword(xmm, xmm_rm, imm8); }
    }
    CMPPD {
        execute: compare_packed_mask::<_>;
        forms { P66 0x0f 0xc2 => qword(xmm, xmm_rm, imm8); }
    }
}

fn compare_scalar_mask<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    predicate: Input<I8>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let predicate = predicate.read(execution)?;
    let controls = execution.simd_controls()?;
    floating::scalar_binary::<T>(execution, destination, source, |_, left, right| {
        Ok(
            FloatOperands::new(left, right, controls.denormals_are_zero())
                .compare_mask(predicate.clone()),
        )
    })
}

fn compare_packed_mask<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    predicate: Input<I8>,
) -> Result<(), BuildError> {
    let predicate = predicate.read(execution)?;
    let controls = execution.simd_controls()?;
    floating::packed_binary(execution, destination, source, |_, left, right| {
        let left = PackedFloatOperand::new(T::FORMAT, left, controls.denormals_are_zero());
        let right = PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero());
        Ok(left.compare_mask(&right, predicate))
    })
}

fn compare_flags<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    signal_quiet_nan: bool,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    let right = source.read::<T>(execution, VectorAlignment::Unaligned)?;
    let left = T::read_register(execution, destination)?;
    let result = FloatOperands::new(left, right, controls.denormals_are_zero())
        .compare(signal_quiet_nan.into());
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_flags(FlagChange::partial([
        (Flag::CF, result.unordered.or(result.less)),
        (Flag::PF, result.unordered.clone()),
        (Flag::ZF, result.unordered.or(result.equal)),
        (Flag::OF, false.into()),
        (Flag::SF, false.into()),
        (Flag::AF, false.into()),
    ]))
}
