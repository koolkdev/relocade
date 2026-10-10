//! SSE and SSE2 moves and logical operations preserve raw XMM encodings.

use super::*;
use crate::instruction::{UpperBits, VectorAlignment, XmmLocation};
use crate::memory::TransferType;
use wasm86_compiler::VectorLane;

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
    XORPS {
        execute: xor_vector;
        forms {
            NP 0x0f 0x57 => operands(xmm, xmm_rm);
        }
    }
    XORPD {
        execute: xor_vector;
        forms {
            P66 0x0f 0x57 => operands(xmm, xmm_rm);
        }
    }
}

fn move_vector(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
    alignment: VectorAlignment,
) -> Result<(), BuildError> {
    let value = source.read_vector(execution, alignment)?;
    destination.write_vector(execution, alignment, value)
}

fn move_scalar<T: VectorLane + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
) -> Result<(), BuildError> {
    // MOVSS/MOVSD memory loads clear upper XMM bits;
    // register copies preserve them.
    let upper_bits = if source.is_memory() {
        UpperBits::Clear
    } else {
        UpperBits::Preserve
    };
    let value = source.read_scalar::<T>(execution)?;
    destination.write_scalar(execution, upper_bits, value)
}

fn xor_vector(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let XmmLocation::Register(register) = destination else {
        unreachable!("XOR has an XMM destination")
    };
    let right = source.read_vector(execution, VectorAlignment::Aligned)?;
    let left = execution.read_xmm(register.clone())?;
    execution.write_xmm(register, left.xor(right))
}
