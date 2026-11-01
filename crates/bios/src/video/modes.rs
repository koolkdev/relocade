//! Firmware mode transitions through the same ports and MMIO used by guests.
use super::{
    fonts, graphics, palette, presets, write_crtc_word, write_indexed, ACTIVE_PAGE, COLUMNS,
    CRTC_PORT, CURSORS, CURSOR_SHAPE, LAST_ROW, MODE, MODESET_CONTROL, PAGE_OFFSET, PAGE_SIZE,
    VIDEO_CONTROL, VIDEO_SWITCHES,
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{
    registers::{crtc, sequencer, CRTC_INDEX_PORT, SEQUENCER_INDEX_PORT},
    TEXT_MEMORY_BYTES, TEXT_MEMORY_START,
};

// BIOS mode numbers accepted by INT 10h/AH=00h.
pub(crate) const COLOR_TEXT_80X25: u8 = 0x03;
pub(super) const GRAPHICS_320X200_4: u8 = 0x04;
// VGA has identical output for the original CGA color/burst-disabled pair.
pub(super) const GRAPHICS_320X200_4_ALT: u8 = 0x05;
pub(super) const GRAPHICS_640X200_2: u8 = 0x06;
pub(super) const GRAPHICS_320X200_16: u8 = 0x0d;
pub(super) const GRAPHICS_640X200_16: u8 = 0x0e;
pub(super) const GRAPHICS_640X350_16: u8 = 0x10;
pub(super) const GRAPHICS_640X480_16: u8 = 0x12;
pub(super) const GRAPHICS_320X200_256: u8 = 0x13;

/// Returns the mode-set AL result, or leaves the device untouched if unsupported.
pub(crate) fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
    let mode = request & 0x7f;
    let preset = match mode {
        COLOR_TEXT_80X25 => match memory.read_u8(MODESET_CONTROL).expect("mode-set control") & 0x90
        {
            0 => &presets::TEXT_350,
            0x10 => &presets::TEXT_400,
            _ => &presets::TEXT_200,
        },
        GRAPHICS_320X200_4 | GRAPHICS_320X200_4_ALT => &presets::CGA_320X200,
        GRAPHICS_640X200_2 => &presets::CGA_640X200,
        GRAPHICS_320X200_16 => &presets::PLANAR_320X200,
        GRAPHICS_640X200_16 => &presets::PLANAR_640X200,
        GRAPHICS_640X350_16 => &presets::PLANAR_640X350,
        GRAPHICS_640X480_16 => &presets::PLANAR_640X480,
        GRAPHICS_320X200_256 => &presets::INDEXED256,
        _ => return None,
    };
    let text = mode == COLOR_TEXT_80X25;
    // Restore routing and write masks before any clear. Font loading temporarily
    // selects plane two, then the preset restores the mode's normal aperture.
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::SYNCHRONOUS_RESET,
    );
    fonts::set_mode(memory, io, preset.character_height, text);
    preset.program(io);

    memory.write_u8(MODE, mode).expect("video mode");
    palette::initialize_compatibility(memory, mode);
    memory
        .write_u16(COLUMNS, preset.columns)
        .expect("video columns");
    memory
        .write_u16(PAGE_SIZE, preset.page_bytes)
        .expect("video page size");
    memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    memory.write_u8(ACTIVE_PAGE, 0).expect("active video page");
    let shape = if text {
        u16::from_be_bytes([preset.character_height - 2, preset.character_height - 1])
    } else {
        0
    };
    memory
        .write_u16(CURSOR_SHAPE, shape)
        .expect("video cursor shape");
    write_crtc_word(io, crtc::CURSOR_START, shape);
    write_crtc_word(io, crtc::CURSOR_ADDRESS_HIGH, 0);
    write_crtc_word(io, crtc::START_ADDRESS_HIGH, 0);
    memory
        .write_u16(CRTC_PORT, CRTC_INDEX_PORT)
        .expect("CRTC port");
    memory
        .write(
            LAST_ROW,
            &[preset.character_rows - 1, preset.character_height, 0],
        )
        .expect("video font geometry");
    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    memory
        .write_u8(VIDEO_CONTROL, (control & 0x7f) | (request & 0x80))
        .expect("video control");
    if request & 0x80 == 0 {
        if text {
            let blank = [b' ', 7].repeat(2048);
            for offset in (0..TEXT_MEMORY_BYTES).step_by(blank.len()) {
                memory
                    .write(TEXT_MEMORY_START + offset, &blank)
                    .expect("text page");
            }
        } else {
            graphics::clear(memory);
        }
    }
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::RESET_RELEASED,
    );
    Some(match mode {
        COLOR_TEXT_80X25 | GRAPHICS_320X200_4 | GRAPHICS_320X200_4_ALT => 0x30,
        GRAPHICS_640X200_2 => 0x3f,
        _ => 0x20,
    })
}

/// AH=12h/BL=30h selects a profile for a later mode set; it does not touch VGA.
pub(super) fn select_scanlines(memory: &mut MemoryView<'_>, selection: u8) -> bool {
    let (control_bits, switches) = match selection {
        0 => (0x80, 8), // 200 logical lines, doubled on the VGA raster.
        1 => (0, 9),    // 350 lines.
        2 => (0x10, 9), // 400 lines.
        _ => return false,
    };
    let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
    let previous_switches = memory.read_u8(VIDEO_SWITCHES).expect("video switches");
    memory
        .write_u8(MODESET_CONTROL, (control & !0x90) | control_bits)
        .expect("mode-set control");
    memory
        .write_u8(VIDEO_SWITCHES, (previous_switches & !0x0f) | switches)
        .expect("video switches");
    true
}
