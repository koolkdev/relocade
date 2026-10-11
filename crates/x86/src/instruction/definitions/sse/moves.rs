//! Whole-vector and scalar transfers share typed XMM and checked memory accesses.

use super::*;
use crate::instruction::{VectorAlignment, XmmLocation, XmmType};
use wasm86_compiler::{VectorLane, V128};

instruction_families! {
    MOVUPS {
        execute: move_vector(VectorAlignment::Unaligned);
        forms {
            NP 0x0f 0x10 => operands(xmm, xmm_rm);
            NP 0x0f 0x11 => operands(xmm_rm, xmm);
        }
    }
    MOVUPD {
        execute: move_vector(VectorAlignment::Unaligned);
        forms {
            P66 0x0f 0x10 => operands(xmm, xmm_rm);
            P66 0x0f 0x11 => operands(xmm_rm, xmm);
        }
    }
    MOVDQU {
        execute: move_vector(VectorAlignment::Unaligned);
        forms {
            PF3 0x0f 0x6f => operands(xmm, xmm_rm);
            PF3 0x0f 0x7f => operands(xmm_rm, xmm);
        }
    }
    MOVAPS {
        execute: move_vector(VectorAlignment::Aligned);
        forms {
            NP 0x0f 0x28 => operands(xmm, xmm_rm);
            NP 0x0f 0x29 => operands(xmm_rm, xmm);
        }
    }
    MOVAPD {
        execute: move_vector(VectorAlignment::Aligned);
        forms {
            P66 0x0f 0x28 => operands(xmm, xmm_rm);
            P66 0x0f 0x29 => operands(xmm_rm, xmm);
        }
    }
    MOVDQA {
        execute: move_vector(VectorAlignment::Aligned);
        forms {
            P66 0x0f 0x6f => operands(xmm, xmm_rm);
            P66 0x0f 0x7f => operands(xmm_rm, xmm);
        }
    }
    MOVSS {
        execute: move_scalar::<_>;
        forms {
            PF3 0x0f 0x10 => dword(xmm, xmm_rm);
            PF3 0x0f 0x11 => dword(xmm_rm, xmm);
        }
    }
    MOVSD {
        execute: move_scalar::<_>;
        forms {
            PF2 0x0f 0x10 => qword(xmm, xmm_rm);
            PF2 0x0f 0x11 => qword(xmm_rm, xmm);
        }
    }
}

fn move_vector(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
    alignment: VectorAlignment,
) -> Result<(), BuildError> {
    let value = source.read::<V128>(execution, alignment)?;
    destination.write(execution, alignment, value)
}

fn move_scalar<T: VectorLane + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let memory_load = source.is_memory();
    let value = source.read::<T>(execution, VectorAlignment::Unaligned)?;
    if memory_load {
        // MOVSS/MOVSD memory loads also clear the rest of the XMM register.
        let XmmLocation::Register(register) = destination else {
            unreachable!("MOVSS/MOVSD memory loads have an XMM destination")
        };
        let vector = Val::<V128>::from(0_u128).replace_lane(0, value);
        execution.write_xmm(register, vector)
    } else {
        destination.write(execution, VectorAlignment::Unaligned, value)
    }
}
