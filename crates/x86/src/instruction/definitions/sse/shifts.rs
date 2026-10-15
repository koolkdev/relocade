//! Lane shifts and whole-register byte shifts use distinct unsigned count rules.

use super::*;
use crate::{
    instruction::PackedShiftCount,
    simd::{ByteShiftOp, PackedShiftOp},
};
use wasm86_compiler::{MemoryInt, VectorLane};

instruction_families! {
    PSLLDQ {
        execute: shift_bytes(ByteShiftOp::Left);
        requires: [Sse2];
        forms { P66 0x0f 0x73 / 7 => operands(register(xmm_rm), imm8); }
    }
    PSRLDQ {
        execute: shift_bytes(ByteShiftOp::Right);
        requires: [Sse2];
        forms { P66 0x0f 0x73 / 3 => operands(register(xmm_rm), imm8); }
    }
    PSLLW {
        execute: shift::<_>(PackedShiftOp::Left);
        requires: [Sse2];
        forms {
            P66 0x0f 0xf1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 6 => word(register(xmm_rm), imm8);
        }
    }
    PSLLD {
        execute: shift::<_>(PackedShiftOp::Left);
        requires: [Sse2];
        forms {
            P66 0x0f 0xf2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 6 => dword(register(xmm_rm), imm8);
        }
    }
    PSLLQ {
        execute: shift::<_>(PackedShiftOp::Left);
        requires: [Sse2];
        forms {
            P66 0x0f 0xf3 => qword(xmm, xmm_rm);
            P66 0x0f 0x73 / 6 => qword(register(xmm_rm), imm8);
        }
    }
    PSRLW {
        execute: shift::<_>(PackedShiftOp::RightLogical);
        requires: [Sse2];
        forms {
            P66 0x0f 0xd1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 2 => word(register(xmm_rm), imm8);
        }
    }
    PSRLD {
        execute: shift::<_>(PackedShiftOp::RightLogical);
        requires: [Sse2];
        forms {
            P66 0x0f 0xd2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 2 => dword(register(xmm_rm), imm8);
        }
    }
    PSRLQ {
        execute: shift::<_>(PackedShiftOp::RightLogical);
        requires: [Sse2];
        forms {
            P66 0x0f 0xd3 => qword(xmm, xmm_rm);
            P66 0x0f 0x73 / 2 => qword(register(xmm_rm), imm8);
        }
    }
    PSRAW {
        execute: shift::<_>(PackedShiftOp::RightArithmetic);
        requires: [Sse2];
        forms {
            P66 0x0f 0xe1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 4 => word(register(xmm_rm), imm8);
        }
    }
    PSRAD {
        execute: shift::<_>(PackedShiftOp::RightArithmetic);
        requires: [Sse2];
        forms {
            P66 0x0f 0xe2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 4 => dword(register(xmm_rm), imm8);
        }
    }
}

fn shift_bytes(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    count: Input<I8>,
    operation: ByteShiftOp,
) -> Result<(), BuildError> {
    let count = count.read(execution)?;
    let input = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, operation.apply(input, count))
}

fn shift<T: VectorLane + MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    count: impl PackedShiftCount,
    operation: PackedShiftOp,
) -> Result<(), BuildError> {
    let count = count.read_count(execution)?;
    let input = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, operation.apply::<T, _>(input, count))
}
