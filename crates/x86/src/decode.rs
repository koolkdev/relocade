use crate::{state::Gpr32, BlockError};

pub(super) fn mov32(bytes: &[u8], address: u32) -> Result<(Gpr32, u32), BlockError> {
    let Some(&opcode) = bytes.first() else {
        return Err(BlockError::TruncatedInstruction {
            address,
            available: 0,
        });
    };
    let destination = match opcode {
        0xb8 => Gpr32::Eax,
        0xb9 => Gpr32::Ecx,
        0xba => Gpr32::Edx,
        0xbb => Gpr32::Ebx,
        0xbc => Gpr32::Esp,
        0xbd => Gpr32::Ebp,
        0xbe => Gpr32::Esi,
        0xbf => Gpr32::Edi,
        _ => return Err(BlockError::UnsupportedOpcode { address, opcode }),
    };
    let instruction = bytes.get(..5).ok_or(BlockError::TruncatedInstruction {
        address,
        available: bytes.len(),
    })?;
    let immediate = u32::from_le_bytes([
        instruction[1],
        instruction[2],
        instruction[3],
        instruction[4],
    ]);
    Ok((destination, immediate))
}
