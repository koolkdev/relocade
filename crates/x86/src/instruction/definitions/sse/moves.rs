//! Whole-vector and scalar transfers share typed XMM and checked memory accesses.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::OperandSpan,
    instruction::{VectorAlignment, XmmLocation, XmmType},
    memory::Intent,
    register::{Gpr32, RegisterCode},
};
use wasm86_compiler::{VectorLane, I64, V128};

instruction_families! {
    MOVD_TO_XMM {
        execute: move_dword_to_xmm;
        requires: [Sse2];
        forms { P66 0x0f 0x6e => dword(xmm, rm); }
    }
    MOVD_FROM_XMM {
        execute: move_dword_from_xmm;
        requires: [Sse2];
        forms { P66 0x0f 0x7e => dword(rm, xmm); }
    }
    MOVQ {
        execute: move_quadword;
        requires: [Sse2];
        forms {
            PF3 0x0f 0x7e => operands(xmm, xmm_rm);
            P66 0x0f 0xd6 => operands(xmm_rm, xmm);
        }
    }
    MOVUPS {
        execute: move_vector(VectorAlignment::Unaligned);
        requires: [Sse];
        forms {
            NP 0x0f 0x10 => operands(xmm, xmm_rm);
            NP 0x0f 0x11 => operands(xmm_rm, xmm);
        }
    }
    MOVUPD {
        execute: move_vector(VectorAlignment::Unaligned);
        requires: [Sse2];
        forms {
            P66 0x0f 0x10 => operands(xmm, xmm_rm);
            P66 0x0f 0x11 => operands(xmm_rm, xmm);
        }
    }
    MOVDQU {
        execute: move_vector(VectorAlignment::Unaligned);
        requires: [Sse2];
        forms {
            PF3 0x0f 0x6f => operands(xmm, xmm_rm);
            PF3 0x0f 0x7f => operands(xmm_rm, xmm);
        }
    }
    MOVAPS {
        execute: move_vector(VectorAlignment::Aligned);
        requires: [Sse];
        forms {
            NP 0x0f 0x28 => operands(xmm, xmm_rm);
            NP 0x0f 0x29 => operands(xmm_rm, xmm);
        }
    }
    MOVAPD {
        execute: move_vector(VectorAlignment::Aligned);
        requires: [Sse2];
        forms {
            P66 0x0f 0x28 => operands(xmm, xmm_rm);
            P66 0x0f 0x29 => operands(xmm_rm, xmm);
        }
    }
    MOVDQA {
        execute: move_vector(VectorAlignment::Aligned);
        requires: [Sse2];
        forms {
            P66 0x0f 0x6f => operands(xmm, xmm_rm);
            P66 0x0f 0x7f => operands(xmm_rm, xmm);
        }
    }
    MOVSS {
        execute: move_scalar::<_>;
        requires: [Sse];
        forms {
            PF3 0x0f 0x10 => dword(xmm, xmm_rm);
            PF3 0x0f 0x11 => dword(xmm_rm, xmm);
        }
    }
    MOVSD {
        execute: move_scalar::<_>;
        requires: [Sse2];
        forms {
            PF2 0x0f 0x10 => qword(xmm, xmm_rm);
            PF2 0x0f 0x11 => qword(xmm_rm, xmm);
        }
    }
    MOVNTPS {
        execute: store_non_temporal;
        requires: [Sse];
        forms { NP 0x0f 0x2b => operands(mem, xmm); }
    }
    MOVNTPD {
        execute: store_non_temporal;
        requires: [Sse2];
        forms { P66 0x0f 0x2b => operands(mem, xmm); }
    }
    MOVNTDQ {
        execute: store_non_temporal;
        requires: [Sse2];
        forms { P66 0x0f 0xe7 => operands(mem, xmm); }
    }
    MASKMOVDQU {
        execute: store_masked;
        effects: [memory_write];
        requires: [Sse2];
        forms { P66 0x0f 0xf7 => operands(xmm, register(xmm_rm)); }
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

fn store_non_temporal(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
    source: RegisterCode,
) -> Result<(), BuildError> {
    let value = execution.read_xmm(source)?;
    execution
        .memory_operand(
            destination,
            OperandSpan::aligned(16, 16),
            Intent::Write,
            &[],
        )?
        .write(execution, 0, value)
}

fn store_masked(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: RegisterCode,
    mask: RegisterCode,
) -> Result<(), BuildError> {
    let value = execution.read_xmm(source)?;
    let mask = execution.read_xmm(mask)?;
    let address = execution.memory_address_at_register(Gpr32::Edi, execution.data_segment());
    execution.write_masked_memory(address, value, mask)
}
