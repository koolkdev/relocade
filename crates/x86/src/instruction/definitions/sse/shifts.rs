//! Packed shifts use the low qword count or an unsigned immediate byte.

use super::*;
use crate::{alu::ShiftOp, instruction::PackedShiftCount};
use wasm86_compiler::{MemoryInt, VectorLane};

instruction_families! {
    PSLLW {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            P66 0x0f 0xf1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 6 => word(register(xmm_rm), imm8);
        }
    }
    PSLLD {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            P66 0x0f 0xf2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 6 => dword(register(xmm_rm), imm8);
        }
    }
    PSLLQ {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            P66 0x0f 0xf3 => qword(xmm, xmm_rm);
            P66 0x0f 0x73 / 6 => qword(register(xmm_rm), imm8);
        }
    }
    PSRLW {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            P66 0x0f 0xd1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 2 => word(register(xmm_rm), imm8);
        }
    }
    PSRLD {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            P66 0x0f 0xd2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 2 => dword(register(xmm_rm), imm8);
        }
    }
    PSRLQ {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            P66 0x0f 0xd3 => qword(xmm, xmm_rm);
            P66 0x0f 0x73 / 2 => qword(register(xmm_rm), imm8);
        }
    }
    PSRAW {
        execute: shift::<_>(ShiftOp::RightArithmetic);
        forms {
            P66 0x0f 0xe1 => word(xmm, xmm_rm);
            P66 0x0f 0x71 / 4 => word(register(xmm_rm), imm8);
        }
    }
    PSRAD {
        execute: shift::<_>(ShiftOp::RightArithmetic);
        forms {
            P66 0x0f 0xe2 => dword(xmm, xmm_rm);
            P66 0x0f 0x72 / 4 => dword(register(xmm_rm), imm8);
        }
    }
}

fn shift<T: VectorLane + MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    count: impl PackedShiftCount,
    operation: ShiftOp,
) -> Result<(), BuildError> {
    let count = count.read_count(execution)?;
    let input = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, operation.apply_packed::<T, _>(input, count))
}
