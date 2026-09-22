//! Architectural x87 fixtures share raw values, stack images and status expectations.

use wasm86_x86::{CpuState, StoredX87Control, StoredX87Status};

use super::machine::{Exit, Image, Step};

pub(crate) fn set_control(control: &mut StoredX87Control, word: u16) {
    *control = StoredX87Control {
        invalid_mask: (word & 1) as u8,
        denormal_mask: ((word >> 1) & 1) as u8,
        zero_divide_mask: ((word >> 2) & 1) as u8,
        overflow_mask: ((word >> 3) & 1) as u8,
        underflow_mask: ((word >> 4) & 1) as u8,
        precision_mask: ((word >> 5) & 1) as u8,
        precision_control: ((word >> 8) & 3) as u8,
        rounding_control: ((word >> 10) & 3) as u8,
        infinity_control: ((word >> 12) & 1) as u8,
        reserved: control.reserved,
        reserved_bits: word & 0xe0c0,
    };
}

pub(crate) const fn status(word: u16) -> StoredX87Status {
    StoredX87Status {
        invalid: (word & 1) as u8,
        denormal: ((word >> 1) & 1) as u8,
        zero_divide: ((word >> 2) & 1) as u8,
        overflow: ((word >> 3) & 1) as u8,
        underflow: ((word >> 4) & 1) as u8,
        precision: ((word >> 5) & 1) as u8,
        stack_fault: ((word >> 6) & 1) as u8,
        top: ((word >> 11) & 7) as u8,
        c0: ((word >> 8) & 1) as u8,
        c1: ((word >> 9) & 1) as u8,
        c2: ((word >> 10) & 1) as u8,
        c3: ((word >> 14) & 1) as u8,
        error_summary: ((word >> 7) & 1) as u8,
        busy: ((word >> 15) & 1) as u8,
    }
}

pub(crate) const INDEFINITE: (u64, u16) = (0xc000_0000_0000_0000, 0xffff);

pub(crate) fn stack_image(code: &[u8], top: u8, tags: u16) -> Image {
    let mut image = Image::new(code);
    image.cpu.segments.cs.selector = 0x1b;
    image.cpu.segments.ds.selector = 0x23;
    set_control(&mut image.cpu.x87.control, 0x037f);
    image.cpu.x87.status = status(0x4720);
    image.cpu.x87.status.top = top;
    image.cpu.x87.tag_word = tags;
    image.cpu.x87.opcode = 0x0654;
    image.cpu.x87.instruction_offset = 0x1234_5678;
    image.cpu.x87.instruction_selector = 0x17;
    image.cpu.x87.data_offset = 0x89ab_cdef;
    image.cpu.x87.data_selector = 0x27;
    for (index, register) in image.cpu.x87.registers.iter_mut().enumerate() {
        register.significand = 0x8000_0000_0000_0000 + index as u64 * 0x1000_0001;
        register.sign_exponent = 0x3fff + index as u16;
        register.reserved.fill(0x80 + index as u8);
    }
    image
}

pub(crate) fn register_bits(cpu: &CpuState, physical: usize) -> (u64, u16) {
    let register = cpu.x87.registers[physical];
    (register.significand, register.sign_exponent)
}

pub(crate) fn write_register_bits(cpu: &mut CpuState, physical: usize, value: (u64, u16)) {
    // Padding belongs to the physical snapshot slot, not to an x87 value.
    cpu.x87.registers[physical].significand = value.0;
    cpu.x87.registers[physical].sign_exponent = value.1;
}

pub(crate) fn complete_x87(mut cpu: CpuState, bytes: u32, opcode: u16) -> CpuState {
    cpu.x87.instruction_offset = cpu.eip;
    cpu.x87.instruction_selector = 0x1b;
    // This emulator enables the P4 opcode-compatibility policy. Register-only
    // operations retain the architecturally undefined data pointer.
    cpu.x87.opcode = opcode;
    cpu.eip = cpu.eip.wrapping_add(bytes);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

pub(crate) fn dispatch(cpu: CpuState) -> Step<'static> {
    Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    }
}

pub(crate) fn real80(value: (u64, u16)) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[..8].copy_from_slice(&value.0.to_le_bytes());
    bytes[8..].copy_from_slice(&value.1.to_le_bytes());
    bytes
}
