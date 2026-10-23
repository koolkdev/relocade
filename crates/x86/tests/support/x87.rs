//! Architectural x87 fixtures share raw values, stack images and status expectations.

pub(crate) mod arithmetic;

use relocade_x86::{CpuState, X87Control, X87Status};

use super::machine::{retire, Image};

pub(crate) fn set_control(control: &mut X87Control, word: u16) {
    *control = X87Control {
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

pub(crate) const fn status(word: u16) -> X87Status {
    X87Status {
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

pub(crate) fn store_operand_image(
    code: &[u8],
    source: (u64, u16),
    control: u16,
    empty: bool,
) -> Image {
    let mut image = stack_image(code, 7, 0xffff);
    image.cpu.x87.status.precision = 0;
    set_control(&mut image.cpu.x87.control, control);
    if empty {
        // Empty slots retain a payload so a mistaken read remains observable.
        write_register_bits(&mut image.cpu, 7, source);
    } else {
        write_value(&mut image.cpu, 7, source);
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
    retire(cpu, bytes as usize)
}

/// Checkpoint after a six-byte absolute memory load of a literal extended value.
pub(crate) fn loaded_memory_operand(
    cpu: CpuState,
    address: u32,
    value: (u64, u16),
    opcode: u16,
) -> CpuState {
    let mut cpu = complete_x87(cpu, 6, opcode);
    cpu.x87.status.top = cpu.x87.status.top.wrapping_sub(1) & 7;
    cpu.x87.status.c1 = 0;
    cpu.x87.data_offset = address;
    cpu.x87.data_selector = 0x23;
    let top = cpu.x87.status.top;
    write_value(&mut cpu, usize::from(top), value);
    cpu
}

pub(crate) fn real80(value: (u64, u16)) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[..8].copy_from_slice(&value.0.to_le_bytes());
    bytes[8..].copy_from_slice(&value.1.to_le_bytes());
    bytes
}

fn tag(value: (u64, u16)) -> u16 {
    match (value.0, value.1 & 0x7fff) {
        (0, 0) => 1,
        (_, 0 | 0x7fff) => 2,
        (significand, _) if significand < (1_u64 << 63) => 2,
        _ => 0,
    }
}

pub(crate) fn write_value(cpu: &mut CpuState, slot: usize, value: (u64, u16)) {
    write_register_bits(cpu, slot, value);
    cpu.x87.tag_word = (cpu.x87.tag_word & !(3 << (slot * 2))) | (tag(value) << (slot * 2));
}
