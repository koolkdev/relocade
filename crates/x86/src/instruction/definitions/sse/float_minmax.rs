//! Legacy floating-point minima and maxima select the source on ties or NaNs.

use super::*;
use crate::simd::FloatMinMax;
use crate::{floating::BinaryEncoding, instruction::XmmType};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    MINSS {
        execute: scalar::<_>(FloatMinMax::Min);
        forms { PF3 0x0f 0x5d => dword(xmm, xmm_rm); }
    }
    MINSD {
        execute: scalar::<_>(FloatMinMax::Min);
        forms { PF2 0x0f 0x5d => qword(xmm, xmm_rm); }
    }
    MINPS {
        execute: packed::<_>(FloatMinMax::Min);
        forms { NP 0x0f 0x5d => dword(xmm, xmm_rm); }
    }
    MINPD {
        execute: packed::<_>(FloatMinMax::Min);
        forms { P66 0x0f 0x5d => qword(xmm, xmm_rm); }
    }
    MAXSS {
        execute: scalar::<_>(FloatMinMax::Max);
        forms { PF3 0x0f 0x5f => dword(xmm, xmm_rm); }
    }
    MAXSD {
        execute: scalar::<_>(FloatMinMax::Max);
        forms { PF2 0x0f 0x5f => qword(xmm, xmm_rm); }
    }
    MAXPS {
        execute: packed::<_>(FloatMinMax::Max);
        forms { NP 0x0f 0x5f => dword(xmm, xmm_rm); }
    }
    MAXPD {
        execute: packed::<_>(FloatMinMax::Max);
        forms { P66 0x0f 0x5f => qword(xmm, xmm_rm); }
    }
}

fn scalar<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatMinMax,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    floating::scalar_binary::<T>(execution, destination, source, |operands| {
        operands.min_max(operation)
    })
}

fn packed<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatMinMax,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    floating::packed_binary::<T>(execution, destination, source, |left, right| {
        left.min_max(&right, operation)
    })
}
