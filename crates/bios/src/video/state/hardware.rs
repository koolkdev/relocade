//! Hardware register block and the four CPU memory-read latches.
use super::super::{crtc_port, input_status_1_port, palette, read_indexed, write_indexed};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{
    crtc, graphics, sequencer, ATTRIBUTE_ADDRESS_PORT, FEATURE_CONTROL_READ_PORT,
    GRAPHICS_INDEX_PORT, MISC_OUTPUT_READ_PORT, MISC_OUTPUT_WRITE_PORT, SEQUENCER_INDEX_PORT,
};

pub(super) const SIZE: usize = 0x46;
const LATCH_SCRATCH: u32 = 0xaffff;

pub(super) fn save(memory: &mut MemoryView<'_>, io: &IoBus) -> [u8; SIZE] {
    let mut bytes = [0; SIZE];
    let crtc = crtc_port(io);
    for (byte, port) in bytes[..5].iter_mut().zip([
        SEQUENCER_INDEX_PORT,
        crtc,
        GRAPHICS_INDEX_PORT,
        ATTRIBUTE_ADDRESS_PORT,
        FEATURE_CONTROL_READ_PORT,
    ]) {
        *byte = io.read_port(port, 1) as u8;
    }
    for (index, byte) in bytes[5..9].iter_mut().enumerate() {
        *byte = read_indexed(io, SEQUENCER_INDEX_PORT, index as u8 + 1);
    }
    bytes[9] = io.read_port(MISC_OUTPUT_READ_PORT, 1) as u8;
    for (index, byte) in bytes[0x0a..0x23].iter_mut().enumerate() {
        *byte = read_indexed(io, crtc, index as u8);
    }
    for (index, byte) in bytes[0x23..0x37].iter_mut().enumerate() {
        *byte = palette::read_attribute(io, index as u8);
    }
    for (index, byte) in bytes[0x37..0x40].iter_mut().enumerate() {
        *byte = read_indexed(io, GRAPHICS_INDEX_PORT, index as u8);
    }
    bytes[0x40..0x42].copy_from_slice(&crtc.to_le_bytes());

    // IBM VGA Function p. 2-98: write mode 1 spills the existing latches without
    // reading VRAM first. The last byte in each plane is BIOS scratch space.
    select_latch_memory(io);
    write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, 15);
    write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE, 1);
    memory
        .write_u8(LATCH_SCRATCH, 0)
        .expect("VGA latch scratch");
    for (plane, byte) in bytes[0x42..].iter_mut().enumerate() {
        write_indexed(
            io,
            GRAPHICS_INDEX_PORT,
            graphics::READ_MAP_SELECT,
            plane as u8,
        );
        *byte = memory.read_u8(LATCH_SCRATCH).expect("VGA latch scratch");
    }
    for register in [sequencer::MAP_MASK, sequencer::MEMORY_MODE] {
        write_indexed(
            io,
            SEQUENCER_INDEX_PORT,
            register,
            bytes[4 + register as usize],
        );
    }
    for register in [
        graphics::READ_MAP_SELECT,
        graphics::MODE,
        graphics::MISCELLANEOUS,
    ] {
        write_indexed(
            io,
            GRAPHICS_INDEX_PORT,
            register,
            bytes[0x37 + register as usize],
        );
    }
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(bytes[9]));
    io.write_port(SEQUENCER_INDEX_PORT, 1, u32::from(bytes[0]));
    io.write_port(crtc, 1, u32::from(bytes[1]));
    io.write_port(GRAPHICS_INDEX_PORT, 1, u32::from(bytes[2]));
    bytes
}

fn select_latch_memory(io: &IoBus) {
    let miscellaneous = io.read_port(MISC_OUTPUT_READ_PORT, 1);
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, miscellaneous | 2);
    write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MEMORY_MODE, 6);
    write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MISCELLANEOUS, 4);
}

pub(super) fn restore(memory: &mut MemoryView<'_>, io: &IoBus, bytes: &[u8; SIZE]) {
    select_latch_memory(io);
    for (register, value) in [
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::MODE, 0),
        (graphics::BIT_MASK, 0xff),
    ] {
        write_indexed(io, GRAPHICS_INDEX_PORT, register, value);
    }
    for (plane, &byte) in bytes[0x42..].iter().enumerate() {
        write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, 1 << plane);
        memory
            .write_u8(LATCH_SCRATCH, byte)
            .expect("VGA latch scratch");
    }
    memory.read_u8(LATCH_SCRATCH).expect("VGA latch scratch");

    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::SYNCHRONOUS_RESET,
    );
    for (index, &byte) in bytes[5..9].iter().enumerate() {
        write_indexed(io, SEQUENCER_INDEX_PORT, index as u8 + 1, byte);
    }
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(bytes[9]));
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::RESET_RELEASED,
    );

    let crtc = u16::from_le_bytes([bytes[0x40], bytes[0x41]]);
    // Unlock 00h–07h before loading timing; the loop then restores protection.
    write_indexed(
        io,
        crtc,
        crtc::VERTICAL_RETRACE_END,
        bytes[0x1b] & !crtc::PROTECT_TIMING_REGISTERS,
    );
    for (index, &byte) in bytes[0x0a..0x23].iter().enumerate() {
        write_indexed(io, crtc, index as u8, byte);
    }
    for (index, &byte) in bytes[0x23..0x37].iter().enumerate() {
        palette::write_attribute(io, index as u8, byte);
    }
    for (index, &byte) in bytes[0x37..0x40].iter().enumerate() {
        write_indexed(io, GRAPHICS_INDEX_PORT, index as u8, byte);
    }
    io.write_port(input_status_1_port(io), 1, u32::from(bytes[4]));
    io.write_port(SEQUENCER_INDEX_PORT, 1, u32::from(bytes[0]));
    io.write_port(crtc, 1, u32::from(bytes[1]));
    io.write_port(GRAPHICS_INDEX_PORT, 1, u32::from(bytes[2]));
    io.read_port(input_status_1_port(io), 1);
    io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(bytes[3]));
}
