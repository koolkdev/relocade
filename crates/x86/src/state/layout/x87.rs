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
/// TOP uses bits 0–2, and every other field uses bit 0.
/// Serialization preserves every byte without normalizing unused bits.
/// This record is not the architectural status-word encoding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredX87Status {
    pub invalid: u8,
    pub denormal: u8,
    pub zero_divide: u8,
    pub overflow: u8,
    pub underflow: u8,
    pub precision: u8,
    pub stack_fault: u8,
    pub top: u8,
    pub c0: u8,
    pub c1: u8,
    pub c2: u8,
    pub c3: u8,
    pub error_summary: u8,
    pub busy: u8,
}

/// Independent control fields. Masks and infinity control use bit 0;
/// precision and rounding control use bits 0–1. `reserved_bits` retains
/// architectural control-word bits selected by 0xe0c0. The codec preserves
/// unused bits too; this record is not a packed architectural control word.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredX87Control {
    pub invalid_mask: u8,
    pub denormal_mask: u8,
    pub zero_divide_mask: u8,
    pub overflow_mask: u8,
    pub underflow_mask: u8,
    pub precision_mask: u8,
    pub precision_control: u8,
    pub rounding_control: u8,
    pub infinity_control: u8,
    /// Snapshot padding, retained without interpretation.
    pub reserved: u8,
    pub reserved_bits: u16,
}

impl Default for StoredX87Control {
    fn default() -> Self {
        Self {
            invalid_mask: 1,
            denormal_mask: 1,
            zero_divide_mask: 1,
            overflow_mask: 1,
            underflow_mask: 1,
            precision_mask: 1,
            precision_control: 3,
            rounding_control: 0,
            infinity_control: 0,
            reserved: 0,
            reserved_bits: 0x0040,
        }
    }
}

/// The x87 environment and eight physical registers in host snapshot order.
/// `registers[0]` is R0; `status.top` maps logical ST(i) to Rn.
/// `tag_word` contains two bits for each physical register, with 3 meaning empty.
/// Pointer offsets and selectors describe the last recorded x87 instructions
/// and data operands. This layout is not an FSAVE or FXSAVE memory image.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredX87 {
    pub control: StoredX87Control,
    pub tag_word: u16,
    pub opcode: u16,
    pub instruction_offset: u32,
    pub data_offset: u32,
    pub instruction_selector: u16,
    pub data_selector: u16,
    pub status: StoredX87Status,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 6],
    pub registers: [StoredX87Register; 8],
}

impl Default for StoredX87 {
    fn default() -> Self {
        Self {
            control: StoredX87Control::default(),
            tag_word: 0xffff,
            opcode: 0,
            instruction_offset: 0,
            data_offset: 0,
            instruction_selector: 0,
            data_selector: 0,
            status: StoredX87Status::default(),
            reserved: [0; 6],
            registers: [StoredX87Register::default(); 8],
        }
    }
}

impl StoredX87 {
    pub(super) fn read(bytes: &[u8]) -> Self {
        Self {
            control: StoredX87Control {
                invalid_mask: bytes[offset_of!(Self, control.invalid_mask)],
                denormal_mask: bytes[offset_of!(Self, control.denormal_mask)],
                zero_divide_mask: bytes[offset_of!(Self, control.zero_divide_mask)],
                overflow_mask: bytes[offset_of!(Self, control.overflow_mask)],
                underflow_mask: bytes[offset_of!(Self, control.underflow_mask)],
                precision_mask: bytes[offset_of!(Self, control.precision_mask)],
                precision_control: bytes[offset_of!(Self, control.precision_control)],
                rounding_control: bytes[offset_of!(Self, control.rounding_control)],
                infinity_control: bytes[offset_of!(Self, control.infinity_control)],
                reserved: bytes[offset_of!(Self, control.reserved)],
                reserved_bits: read_u16(bytes, offset_of!(Self, control.reserved_bits)),
            },
            tag_word: read_u16(bytes, offset_of!(Self, tag_word)),
            opcode: read_u16(bytes, offset_of!(Self, opcode)),
            instruction_offset: read_u32(bytes, offset_of!(Self, instruction_offset)),
            data_offset: read_u32(bytes, offset_of!(Self, data_offset)),
            instruction_selector: read_u16(bytes, offset_of!(Self, instruction_selector)),
            data_selector: read_u16(bytes, offset_of!(Self, data_selector)),
            status: StoredX87Status {
                invalid: bytes[offset_of!(Self, status.invalid)],
                denormal: bytes[offset_of!(Self, status.denormal)],
                zero_divide: bytes[offset_of!(Self, status.zero_divide)],
                overflow: bytes[offset_of!(Self, status.overflow)],
                underflow: bytes[offset_of!(Self, status.underflow)],
                precision: bytes[offset_of!(Self, status.precision)],
                stack_fault: bytes[offset_of!(Self, status.stack_fault)],
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
            (
                offset_of!(Self, control.reserved_bits),
                self.control.reserved_bits,
            ),
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
        for (offset, value) in [
            (
                offset_of!(Self, control.invalid_mask),
                self.control.invalid_mask,
            ),
            (
                offset_of!(Self, control.denormal_mask),
                self.control.denormal_mask,
            ),
            (
                offset_of!(Self, control.zero_divide_mask),
                self.control.zero_divide_mask,
            ),
            (
                offset_of!(Self, control.overflow_mask),
                self.control.overflow_mask,
            ),
            (
                offset_of!(Self, control.underflow_mask),
                self.control.underflow_mask,
            ),
            (
                offset_of!(Self, control.precision_mask),
                self.control.precision_mask,
            ),
            (
                offset_of!(Self, control.precision_control),
                self.control.precision_control,
            ),
            (
                offset_of!(Self, control.rounding_control),
                self.control.rounding_control,
            ),
            (
                offset_of!(Self, control.infinity_control),
                self.control.infinity_control,
            ),
            (offset_of!(Self, control.reserved), self.control.reserved),
            (offset_of!(Self, status.invalid), self.status.invalid),
            (offset_of!(Self, status.denormal), self.status.denormal),
            (
                offset_of!(Self, status.zero_divide),
                self.status.zero_divide,
            ),
            (offset_of!(Self, status.overflow), self.status.overflow),
            (offset_of!(Self, status.underflow), self.status.underflow),
            (offset_of!(Self, status.precision), self.status.precision),
            (
                offset_of!(Self, status.stack_fault),
                self.status.stack_fault,
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
