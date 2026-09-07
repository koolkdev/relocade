use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I8};

use crate::{
    fetch,
    instruction::{DecodedInstruction, Encoding, MOV_IMMEDIATE},
    memory::{DirectRange, Memory},
    register::{Gpr32, Register32},
    state::exit,
    BlockError,
};

pub(super) fn snapshot(
    bytes: &[u8],
    address: u32,
) -> Result<(DecodedInstruction<u32, u32>, &[u8]), BlockError> {
    let Some(&opcode) = bytes.first() else {
        return Err(BlockError::TruncatedInstruction {
            address,
            available: 0,
        });
    };
    let form = &MOV_IMMEDIATE;
    if opcode & form.mask != form.opcode {
        return Err(BlockError::UnsupportedOpcode { address, opcode });
    }
    let length = form.encoding.length() as usize;
    let instruction = bytes
        .get(..length)
        .ok_or(BlockError::TruncatedInstruction {
            address,
            available: bytes.len(),
        })?;
    let (register, immediate) = match form.encoding {
        Encoding::OpcodeRegisterImmediate32 => {
            let offset = form.encoding.immediate_offset() as usize;
            let immediate = u32::from_le_bytes(instruction[offset..].try_into().unwrap());
            (Gpr32::from_code(opcode).into(), immediate)
        }
    };
    let next_eip = address.wrapping_add(form.encoding.length());
    Ok((form.bind(register, immediate, next_eip), &bytes[length..]))
}

pub(super) fn direct_window(
    body: &mut FunctionBuilder<'_>,
    memory: Memory,
    start: &Val<I32>,
) -> Result<DirectRange, BuildError> {
    memory.direct(body, start, MOV_IMMEDIATE.encoding.length())
}

/// `physical` supplies a proven contiguous instruction window. Without it,
/// instruction reads are checked in order before the instruction is lowered.
pub(super) fn live(
    body: &mut FunctionBuilder<'_>,
    memory: Memory,
    start: &Val<I32>,
    physical: Option<&Val<I32>>,
) -> Result<DecodedInstruction<Val<I32>, Val<I32>>, BuildError> {
    let form = &MOV_IMMEDIATE;
    let opcode = match physical {
        Some(address) => memory.load::<I8>(body, address, 0)?,
        None => fetch::byte(body, memory, start)?,
    };
    body.if_(
        opcode.and(u32::from(form.mask)).ne(u32::from(form.opcode)),
        |arm| arm.return_(exit::unsupported(start, &opcode)),
    )?;
    let (register, immediate) = match form.encoding {
        Encoding::OpcodeRegisterImmediate32 => {
            let offset = form.encoding.immediate_offset();
            let immediate = match physical {
                Some(address) => memory.load::<I32>(body, address, offset)?,
                None => fetch::immediate32(body, memory, &start.add(offset))?,
            };
            let register = Register32::indexed(opcode.unsigned().extend::<I32>());
            (register, immediate)
        }
    };
    Ok(form.bind(register, immediate, start.add(form.encoding.length())))
}
