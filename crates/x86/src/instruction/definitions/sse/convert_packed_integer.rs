//! Packed floating sources produce signed dwords and clear unused result lanes.

use super::{floating::IntegerRounding, *};
use crate::floating::BinaryEncoding;
use crate::simd::PackedFloatOperand;
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    CVTPS2DQ {
        execute: convert::<_>(IntegerRounding::Mxcsr);
        forms { P66 0x0f 0x5b => dword(xmm, xmm_rm); }
    }
    CVTPD2DQ {
        execute: convert::<_>(IntegerRounding::Mxcsr);
        forms { PF2 0x0f 0xe6 => qword(xmm, xmm_rm); }
    }
    CVTTPS2DQ {
        execute: convert::<_>(IntegerRounding::Truncate);
        forms { PF3 0x0f 0x5b => dword(xmm, xmm_rm); }
    }
    CVTTPD2DQ {
        execute: convert::<_>(IntegerRounding::Truncate);
        forms { P66 0x0f 0xe6 => qword(xmm, xmm_rm); }
    }
}

fn convert<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    rounding: IntegerRounding,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    let rounding = rounding.mode(&controls);
    let source = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let result = PackedFloatOperand::new(T::FORMAT, source, controls.denormals_are_zero())
        .to_integer(&rounding);
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}
