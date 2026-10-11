//! Packed counts use the full MMX operand; they never wrap at the lane width.

use super::*;
use crate::alu::ShiftOp;
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

trait ShiftCount {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError>;
}

impl ShiftCount for MmxLocation {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError> {
        self.read(execution)
    }
}

impl ShiftCount for Input<I8> {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError> {
        Ok(self.read(execution)?.unsigned().extend())
    }
}

fn shift<T: VectorLane + MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    count: impl ShiftCount,
    operation: ShiftOp,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let count = count.read_count(execution)?;
        let input = execution.read_mmx(destination.clone())?;
        let vector = Val::<V128>::from(0_u128).replace_lane(0, input);
        // Test before narrowing: a nonzero high dword must not become a small count.
        let in_range = count.unsigned().lt(u64::from(T::BYTES * 8));
        let count = count.truncate::<I32>();
        let result = match operation {
            ShiftOp::Left => in_range.select(vector.shl_lanes::<T>(count), 0_u128),
            ShiftOp::RightLogical => in_range.select(vector.unsigned_shr_lanes::<T>(count), 0_u128),
            ShiftOp::RightArithmetic => {
                vector.signed_shr_lanes::<T>(in_range.select(count, T::BYTES * 8 - 1))
            }
        };
        execution.write_mmx(destination, result.extract_lane::<I64>(0))
    })
}
