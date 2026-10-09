//! SSE and SSE2 moves and logical operations preserve raw XMM encodings.

use super::*;
use crate::instruction::{VectorAlignment, XmmLocation};

instruction_families! {
    moves {
        execute: move_vector;
        forms {
            NP 0x0f 0x10 => operands(xmm, xmm_rm);
            NP 0x0f 0x11 => operands(xmm_rm, xmm);
            P66 0x0f 0x10 => operands(xmm, xmm_rm);
            P66 0x0f 0x11 => operands(xmm_rm, xmm);
        }
    }
    xor {
        execute: xor_vector;
        forms {
            NP 0x0f 0x57 => operands(xmm, xmm_rm);
            P66 0x0f 0x57 => operands(xmm, xmm_rm);
        }
    }
}

fn move_vector(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
) -> Result<(), BuildError> {
    let value = source.read_vector(execution, VectorAlignment::Unaligned)?;
    destination.write_vector(execution, VectorAlignment::Unaligned, value)
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
