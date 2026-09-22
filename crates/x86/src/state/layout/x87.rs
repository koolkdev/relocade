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

/// Independent backing bytes for the x87 status fields, in snapshot order.
/// Exceptions use bits 0–6, TOP uses bits 0–2, and other fields use bit 0.
/// Serialization preserves every byte without normalizing unused bits.
/// This record is not the architectural status-word encoding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredX87Status {
    /// IE, DE, ZE, OE, UE, PE and SF in their architectural bit positions.
    pub exception_flags: u8,
    pub top: u8,
    pub c0: u8,
    pub c1: u8,
    pub c2: u8,
    pub c3: u8,
    pub error_summary: u8,
    pub busy: u8,
}

/// The x87 environment and eight physical registers in host snapshot order.
/// `registers[0]` is R0; `status.top` maps logical ST(i) to Rn.
/// `tag_word` stores all eight architectural two-bit tags, including empty tags.
/// Pointer offsets and selectors describe the last recorded x87 instructions
/// and data operands. This layout is not an FSAVE or FXSAVE memory image.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredX87 {
    pub control_word: u16,
    pub tag_word: u16,
    pub opcode: u16,
    /// Snapshot padding, retained without interpretation.
    pub reserved_control: [u8; 2],
    pub instruction_offset: u32,
    pub data_offset: u32,
    pub instruction_selector: u16,
    pub data_selector: u16,
    pub status: StoredX87Status,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 4],
    pub registers: [StoredX87Register; 8],
}

impl Default for StoredX87 {
    fn default() -> Self {
        Self {
            control_word: 0x037f,
            tag_word: 0xffff,
            opcode: 0,
            reserved_control: [0; 2],
            instruction_offset: 0,
            data_offset: 0,
            instruction_selector: 0,
            data_selector: 0,
            status: StoredX87Status::default(),
            reserved: [0; 4],
            registers: [StoredX87Register::default(); 8],
        }
    }
}

impl StoredX87 {
    pub(super) fn read(bytes: &[u8]) -> Self {
        Self {
            control_word: read_u16(bytes, offset_of!(Self, control_word)),
            tag_word: read_u16(bytes, offset_of!(Self, tag_word)),
            opcode: read_u16(bytes, offset_of!(Self, opcode)),
            reserved_control: read(bytes, offset_of!(Self, reserved_control)),
            instruction_offset: read_u32(bytes, offset_of!(Self, instruction_offset)),
            data_offset: read_u32(bytes, offset_of!(Self, data_offset)),
            instruction_selector: read_u16(bytes, offset_of!(Self, instruction_selector)),
            data_selector: read_u16(bytes, offset_of!(Self, data_selector)),
            status: StoredX87Status {
                exception_flags: bytes[offset_of!(Self, status.exception_flags)],
                top: bytes[offset_of!(Self, status.top)],
                c0: bytes[offset_of!(Self, status.c0)],
                c1: bytes[offset_of!(Self, status.c1)],
                c2: bytes[offset_of!(Self, status.c2)],
                c3: bytes[offset_of!(Self, status.c3)],
                error_summary: bytes[offset_of!(Self, status.error_summary)],
                busy: bytes[offset_of!(Self, status.busy)],
            },
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
            offset_of!(Self, reserved_control),
            &self.reserved_control,
        );
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
        for (offset, value) in [
            (
                offset_of!(Self, status.exception_flags),
                self.status.exception_flags,
            ),
            (offset_of!(Self, status.top), self.status.top),
            (offset_of!(Self, status.c0), self.status.c0),
            (offset_of!(Self, status.c1), self.status.c1),
            (offset_of!(Self, status.c2), self.status.c2),
            (offset_of!(Self, status.c3), self.status.c3),
            (
                offset_of!(Self, status.error_summary),
                self.status.error_summary,
            ),
            (offset_of!(Self, status.busy), self.status.busy),
        ] {
            bytes[offset] = value;
        }
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
