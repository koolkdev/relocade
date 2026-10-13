//! Scalar comparisons publish MXCSR before changing EFLAGS.

use super::*;
use crate::{
    flags::{Flag, FlagChange},
    instruction::XmmType,
    simd::{FloatLane, FloatOperands},
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
}

fn compare_flags<T: FloatLane + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    signal_quiet_nan: bool,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let right = source.read::<T>(execution, VectorAlignment::Unaligned)?;
    let left = T::read_register(execution, destination)?;
    let controls = execution.read_mxcsr()?;
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
