//! Whole-vector and scalar transfers share typed XMM and checked memory accesses.

use super::*;
use crate::instruction::{VectorAlignment, XmmLocation, XmmType};
use crate::register::RegisterCode;
use wasm86_compiler::{VectorLane, I64, V128};

instruction_families! {
    MOVD_TO_XMM {
        execute: move_dword_to_xmm;
        forms { P66 0x0f 0x6e => dword(xmm, rm); }
    }
    MOVD_FROM_XMM {
        execute: move_dword_from_xmm;
        forms { P66 0x0f 0x7e => dword(rm, xmm); }
    }
    MOVQ {
        execute: move_quadword;
        forms {
            PF3 0x0f 0x7e => operands(xmm, xmm_rm);
            P66 0x0f 0xd6 => operands(xmm_rm, xmm);
        }
    }
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

fn move_dword_to_xmm(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: Input<I32>,
) -> Result<(), BuildError> {
    let value = source.read(execution)?;
    let vector = Val::<V128>::from(0_u128).replace_lane(0, value);
    execution.write_xmm(destination, vector)
}

fn move_dword_from_xmm(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: RegisterCode,
) -> Result<(), BuildError> {
    let value = execution.read_xmm(source)?.extract_lane::<I32>(0);
    destination.write(execution, value)
}

fn move_quadword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let value = source.read::<I64>(execution, VectorAlignment::Unaligned)?;
    match destination {
        XmmLocation::Register(register) => {
            // Both MOVQ encodings clear the high half, including register copies.
            let vector = Val::<V128>::from(0_u128).replace_lane(0, value);
            execution.write_xmm(register, vector)
        }
        memory => memory.write(execution, VectorAlignment::Unaligned, value),
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
        let vector = Val::<V128>::from(0_u128).replace_lane(0, value);
        destination.write(execution, VectorAlignment::Unaligned, vector)
    } else {
        destination.write(execution, VectorAlignment::Unaligned, value)
    }
}
