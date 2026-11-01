//! INT 10h/AH=1Bh: the guest-visible functionality table and 64-byte state report.
use super::{
    adapter, buffer_addresses, modes, palette, read_indexed, CHARACTER_HEIGHT, LAST_ROW, MODE,
    MODESET_CONTROL, VIDEO_CONTROL, VIDEO_SWITCHES,
};
use crate::{screen, BIOS_SEGMENT};
use relocade_machine::{real_mode_interrupt, HookResult, IoBus, MemoryView};
use relocade_vga::registers::{attribute, sequencer, SEQUENCER_INDEX_PORT};

// Follows the font ROM and ninth-column replacement tables.
const CAPABILITIES_OFFSET: u16 = 0xa610;
const CAPABILITIES_POINTER: u32 = ((BIOS_SEGMENT as u32) << 16) | CAPABILITIES_OFFSET as u32;
// 00h–06h: mode bitmap for 00h–07h and 0Dh–13h.
// 07h: 200-, 350- and 400-line text profiles.
// 08h–09h: two simultaneously visible font banks, eight total banks.
// 0Ah–0Bh: font loading, EGA/DAC palettes, palette paging, blink and display combination.
// 0Ch–0Fh: reserved bytes and unsupported save-pointer override tables.
const CAPABILITIES: [u8; 16] = [
    0xff, 0xe0, 0x0f, 0, 0, 0, 0, 7, 2, 8, 0xe4, 0x0c, 0, 0, 0, 0,
];

pub(super) fn initialize(memory: &mut MemoryView<'_>) {
    memory
        .write_backing(
            (u32::from(BIOS_SEGMENT) << 4) + u32::from(CAPABILITIES_OFFSET),
            &CAPABILITIES,
        )
        .expect("BIOS video functionality table");
}

pub(super) fn interrupt(call: &mut crate::BiosContext<'_>) -> HookResult {
    if call.cpu.bx() != 0 {
        // This service reports an unsupported selector through AL, not carry.
        call.cpu.set_al(0);
        return real_mode_interrupt::return_from_interrupt(call.cpu);
    }
    // Collect the fields before writing: ES:DI may overlap BIOS data.
    let bytes = encode_state(&call.memory, call.io);
    let addresses = buffer_addresses(call.cpu.segments().es.base, call.cpu.di(), 64);
    for (address, byte) in addresses.zip(bytes) {
        call.memory
            .write_u8(address, byte)
            .expect("video state buffer");
    }
    call.cpu.set_al(0x1b);
    real_mode_interrupt::return_from_interrupt(call.cpu)
}

fn encode_state(memory: &MemoryView<'_>, io: &IoBus) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..4].copy_from_slice(&CAPABILITIES_POINTER.to_le_bytes());
    // BDA 449h–466h: mode, columns, pages, cursors, CRTC port and CGA controls.
    memory
        .read(MODE, &mut bytes[0x04..0x22])
        .expect("video data");
    let mode = bytes[4];
    // IBM returns the row count here, unlike BDA 484h's last-row index.
    bytes[0x22] = memory
        .read_u8(LAST_ROW)
        .expect("video rows")
        .wrapping_add(1);
    memory
        .read(CHARACTER_HEIGHT, &mut bytes[0x23..0x25])
        .expect("font height");
    bytes[0x25..0x27].copy_from_slice(&adapter::display_codes(memory));
    bytes[0x27..0x29].copy_from_slice(&color_count(mode).to_le_bytes());
    bytes[0x29] = (0..8)
        .take_while(|&page| screen::valid_page(memory, page))
        .count() as u8;
    bytes[0x2a] = scanline_profile(memory, mode);

    let saved_index = io.read_port(SEQUENCER_INDEX_PORT, 1);
    let fonts = read_indexed(io, SEQUENCER_INDEX_PORT, sequencer::CHARACTER_MAP_SELECT);
    io.write_port(SEQUENCER_INDEX_PORT, 1, saved_index);
    bytes[0x2b] = (fonts & 3) | ((fonts & 0x10) >> 2); // Normal font B.
    bytes[0x2c] = ((fonts >> 2) & 3) | ((fonts & 0x20) >> 3); // Alternate font A.

    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    let modeset = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
    let attribute = palette::read_attribute(io, attribute::MODE_CONTROL);
    bytes[0x2d] = (modeset & 15) | (((control & 1) ^ 1) << 4) | ((attribute & 8) << 2);
    bytes[0x31] = adapter::memory_size(memory);
    // Reserved fields and unsupported save-pointer state remain zero.
    bytes
}

fn color_count(mode: u8) -> u16 {
    match mode {
        modes::TEXT_40X25..=modes::COLOR_TEXT_80X25
        | modes::GRAPHICS_320X200_16
        | modes::GRAPHICS_640X200_16
        | modes::GRAPHICS_640X350_16
        | modes::GRAPHICS_640X480_16 => 16,
        modes::GRAPHICS_320X200_4 | modes::GRAPHICS_320X200_4_ALT => 4,
        modes::GRAPHICS_640X200_2 | modes::GRAPHICS_640X480_2 => 2,
        modes::GRAPHICS_320X200_256 => 256,
        _ => 0, // Monochrome modes 07h/0Fh, or an unknown BDA mode.
    }
}

fn scanline_profile(memory: &MemoryView<'_>, mode: u8) -> u8 {
    match mode {
        modes::GRAPHICS_640X350_MONO | modes::GRAPHICS_640X350_16 => 1,
        modes::GRAPHICS_640X480_2 | modes::GRAPHICS_640X480_16 => 3,
        modes::TEXT_40X25..=modes::COLOR_TEXT_80X25 | modes::MONO_TEXT_80X25 => {
            // IBM reports the BIOS profile, not custom CRTC timing or rows × font height.
            let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
            let switches = memory.read_u8(VIDEO_SWITCHES).expect("video switches") & 15;
            if control & 0x10 != 0 {
                2
            } else if matches!(switches, 3 | 9) || mode == modes::MONO_TEXT_80X25 {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}
