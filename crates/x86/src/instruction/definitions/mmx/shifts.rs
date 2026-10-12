//! Packed counts use the full MMX operand; they never wrap at the lane width.

use super::*;
use crate::alu::ShiftOp;
use crate::instruction::PackedShiftCount;
use wasm86_compiler::{MemoryInt, VectorLane};

instruction_families! {
    PSLLW {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            NP 0x0f 0xf1 => word(mm, mm_rm);
            NP 0x0f 0x71 / 6 => word(register(mm_rm), imm8);
        }
    }
    PSLLD {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            NP 0x0f 0xf2 => dword(mm, mm_rm);
            NP 0x0f 0x72 / 6 => dword(register(mm_rm), imm8);
        }
    }
    PSLLQ {
        execute: shift::<_>(ShiftOp::Left);
        forms {
            NP 0x0f 0xf3 => qword(mm, mm_rm);
            NP 0x0f 0x73 / 6 => qword(register(mm_rm), imm8);
        }
    }
    PSRLW {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            NP 0x0f 0xd1 => word(mm, mm_rm);
            NP 0x0f 0x71 / 2 => word(register(mm_rm), imm8);
        }
    }
    PSRLD {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            NP 0x0f 0xd2 => dword(mm, mm_rm);
            NP 0x0f 0x72 / 2 => dword(register(mm_rm), imm8);
        }
    }
    PSRLQ {
        execute: shift::<_>(ShiftOp::RightLogical);
        forms {
            NP 0x0f 0xd3 => qword(mm, mm_rm);
            NP 0x0f 0x73 / 2 => qword(register(mm_rm), imm8);
        }
    }
    PSRAW {
        execute: shift::<_>(ShiftOp::RightArithmetic);
        forms {
            NP 0x0f 0xe1 => word(mm, mm_rm);
            NP 0x0f 0x71 / 4 => word(register(mm_rm), imm8);
        }
    }
    PSRAD {
        execute: shift::<_>(ShiftOp::RightArithmetic);
        forms {
            NP 0x0f 0xe2 => dword(mm, mm_rm);
            NP 0x0f 0x72 / 4 => dword(register(mm_rm), imm8);
        }
    }
}

fn shift<T: VectorLane + MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    count: impl PackedShiftCount,
    operation: ShiftOp,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let count = count.read_count(execution)?;
        let input = execution.read_mmx(destination.clone())?;
        let result = operation.apply_packed::<T, _>(Val::<V64>::from_bits(input), count);
        execution.write_mmx(destination, result.to_bits())
    })
}
