//! Literal x87 backing, independent of the arithmetic representation in a block.

use std::mem::{offset_of, size_of};

use super::codec::{read, read_u16, read_u32, write};

/// One physical x87 register's binary80 encoding in a padded host snapshot slot.
/// The explicit integer bit belongs to `significand`; `sign_exponent` contains
/// the sign and biased exponent exactly as stored in an extended real operand.
/// Empty tags do not erase these bits, and noncanonical encodings are preserved.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredX87Register {
    pub significand: u64,
    pub sign_exponent: u16,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 6],
}

/// The x87 environment and eight physical registers in host snapshot order.
/// `registers[0]` is R0; the status word's TOP field maps logical ST(i) to Rn.
/// `tag_word` stores all eight architectural two-bit tags, including empty tags.
/// Pointer offsets and selectors describe the last recorded x87 instructions
/// and data operands. This layout is not an FSAVE or FXSAVE memory image.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredX87 {
    pub control_word: u16,
    pub status_word: u16,
    pub tag_word: u16,
    pub opcode: u16,
    pub instruction_offset: u32,
    pub data_offset: u32,
    pub instruction_selector: u16,
    pub data_selector: u16,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 4],
    pub registers: [StoredX87Register; 8],
}

impl Default for StoredX87 {
    fn default() -> Self {
        Self {
            control_word: 0x037f,
            status_word: 0,
            tag_word: 0xffff,
            opcode: 0,
            instruction_offset: 0,
            data_offset: 0,
            instruction_selector: 0,
            data_selector: 0,
            reserved: [0; 4],
            registers: [StoredX87Register::default(); 8],
        }
    }
}

impl StoredX87 {
    pub(super) fn read(bytes: &[u8]) -> Self {
        Self {
            control_word: read_u16(bytes, offset_of!(Self, control_word)),
            status_word: read_u16(bytes, offset_of!(Self, status_word)),
            tag_word: read_u16(bytes, offset_of!(Self, tag_word)),
            opcode: read_u16(bytes, offset_of!(Self, opcode)),
            instruction_offset: read_u32(bytes, offset_of!(Self, instruction_offset)),
            data_offset: read_u32(bytes, offset_of!(Self, data_offset)),
            instruction_selector: read_u16(bytes, offset_of!(Self, instruction_selector)),
            data_selector: read_u16(bytes, offset_of!(Self, data_selector)),
            reserved: read(bytes, offset_of!(Self, reserved)),
            registers: std::array::from_fn(|index| {
                let offset = offset_of!(Self, registers) + index * size_of::<StoredX87Register>();
                StoredX87Register {
                    significand: u64::from_le_bytes(read(
                        bytes,
                        offset + offset_of!(StoredX87Register, significand),
                    )),
                    sign_exponent: read_u16(
                        bytes,
                        offset + offset_of!(StoredX87Register, sign_exponent),
                    ),
                    reserved: read(bytes, offset + offset_of!(StoredX87Register, reserved)),
                }
            }),
        }
    }

    pub(super) fn write(&self, bytes: &mut [u8]) {
        for (offset, value) in [
            (offset_of!(Self, control_word), self.control_word),
            (offset_of!(Self, status_word), self.status_word),
            (offset_of!(Self, tag_word), self.tag_word),
            (offset_of!(Self, opcode), self.opcode),
            (
                offset_of!(Self, instruction_selector),
                self.instruction_selector,
            ),
            (offset_of!(Self, data_selector), self.data_selector),
        ] {
            write(bytes, offset, &value.to_le_bytes());
        }
        write(
            bytes,
            offset_of!(Self, instruction_offset),
            &self.instruction_offset.to_le_bytes(),
        );
        write(
            bytes,
            offset_of!(Self, data_offset),
            &self.data_offset.to_le_bytes(),
        );
        write(bytes, offset_of!(Self, reserved), &self.reserved);
        for (index, register) in self.registers.iter().enumerate() {
            let offset = offset_of!(Self, registers) + index * size_of::<StoredX87Register>();
            write(
                bytes,
                offset + offset_of!(StoredX87Register, significand),
                &register.significand.to_le_bytes(),
            );
            write(
                bytes,
                offset + offset_of!(StoredX87Register, sign_exponent),
                &register.sign_exponent.to_le_bytes(),
            );
            write(
                bytes,
                offset + offset_of!(StoredX87Register, reserved),
                &register.reserved,
            );
        }
    }
}
