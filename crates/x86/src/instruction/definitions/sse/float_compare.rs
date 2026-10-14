//! Floating comparisons publish MXCSR before changing EFLAGS or XMM masks.

use super::*;
use crate::{
    flags::{Flag, FlagChange},
    floating::BinaryEncoding,
    instruction::XmmType,
    simd::{FloatOperands, FloatResult, FloatShape},
};
use wasm86_compiler::{AtLeast, I64};

instruction_families! {
    COMISS {
        execute: compare_flags::<_>(true);
        requires: [Sse];
        forms { NP 0x0f 0x2f => dword(xmm, xmm_rm); }
    }
    UCOMISS {
        execute: compare_flags::<_>(false);
        requires: [Sse];
        forms { NP 0x0f 0x2e => dword(xmm, xmm_rm); }
    }
    COMISD {
        execute: compare_flags::<_>(true);
        requires: [Sse2];
        forms { P66 0x0f 0x2f => qword(xmm, xmm_rm); }
    }
    UCOMISD {
        execute: compare_flags::<_>(false);
        requires: [Sse2];
        forms { P66 0x0f 0x2e => qword(xmm, xmm_rm); }
    }
    CMPSS {
        execute: compare_mask::<_>(FloatShape::Scalar);
        requires: [Sse];
        forms { PF3 0x0f 0xc2 => dword(xmm, xmm_rm, imm8); }
    }
    CMPSD {
        execute: compare_mask::<_>(FloatShape::Scalar);
        requires: [Sse2];
        forms { PF2 0x0f 0xc2 => qword(xmm, xmm_rm, imm8); }
    }
    CMPPS {
        execute: compare_mask::<_>(FloatShape::Packed);
        requires: [Sse];
        forms { NP 0x0f 0xc2 => dword(xmm, xmm_rm, imm8); }
    }
    CMPPD {
        execute: compare_mask::<_>(FloatShape::Packed);
        requires: [Sse2];
        forms { P66 0x0f 0xc2 => qword(xmm, xmm_rm, imm8); }
    }
}

fn compare_mask<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    predicate: Input<I8>,
    shape: FloatShape,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let predicate = predicate.read(execution)?;
    floating::binary::<T>(execution, destination, source, shape, |operands| {
        // Predicates 4..7 negate 0..3; LT/LE and their negations signal on QNaN.
        let kind = predicate.and(3);
        let comparison = operands.compare(kind.eq(1).or(kind.eq(2)));
        let matches = kind
            .eq(0)
            .and(&comparison.equal)
            .or(kind.eq(1).and(&comparison.less))
            .or(kind.eq(2).and(comparison.less.or(&comparison.equal)))
            .or(kind.eq(3).and(comparison.unordered));
        FloatResult {
            value: matches.xor(predicate.and(4).ne(0)).select(-1, 0),
            exceptions: comparison.exceptions,
        }
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
