//! Firmware mode transitions through the same ports and MMIO used by guests.
use super::{
    presets, write_crtc_word, write_indexed, ACTIVE_PAGE, COLUMNS, CRTC_PORT, CURSORS,
    CURSOR_SHAPE, LAST_ROW, MODE, PAGE_OFFSET, PAGE_SIZE, VIDEO_CONTROL,
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{
    registers::{
        crtc, graphics, sequencer, text_mode, CRTC_INDEX_PORT, GRAPHICS_INDEX_PORT,
        MISC_OUTPUT_WRITE_PORT, SEQUENCER_INDEX_PORT,
    },
    TEXT_MEMORY_BYTES, TEXT_MEMORY_START,
};

// BIOS mode numbers accepted by INT 10h/AH=00h.
pub(crate) const COLOR_TEXT_80X25: u8 = 0x03;
pub(super) const GRAPHICS_320X200_16: u8 = 0x0d;
pub(super) const GRAPHICS_640X480_16: u8 = 0x12;
pub(super) const GRAPHICS_320X200_256: u8 = 0x13;

/// Returns the mode-set AL result, or leaves the device untouched if unsupported.
pub(crate) fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
    let mode = request & 0x7f;
    let preset = match mode {
        COLOR_TEXT_80X25 => &presets::TEXT,
        GRAPHICS_320X200_16 => &presets::PLANAR_320X200,
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
    if text {
        load_text_font(memory, io);
    }
    preset.program(io);

    memory.write_u8(MODE, mode).expect("video mode");
    memory
        .write_u16(COLUMNS, preset.columns)
        .expect("video columns");
    memory
        .write_u16(PAGE_SIZE, preset.page_bytes)
        .expect("video page size");
    memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    memory.write_u8(ACTIVE_PAGE, 0).expect("active video page");
    let shape = if text { 0x0607 } else { 0 };
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
            let blank = [0; 4096];
            for offset in (0..65536).step_by(blank.len()) {
                memory
                    .write(0xa0000 + offset, &blank)
                    .expect("graphics memory");
            }
        }
    }
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::RESET_RELEASED,
    );
    Some(if text { 0x30 } else { 0x20 })
}

fn load_text_font(memory: &mut MemoryView<'_>, io: &IoBus) {
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(text_mode::MISC_OUTPUT));
    write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, 4);
    write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MEMORY_MODE, 6);
    for (register, value) in [
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::MODE, 0),
        (graphics::MISCELLANEOUS, 5),
        (graphics::BIT_MASK, 0xff),
    ] {
        write_indexed(io, GRAPHICS_INDEX_PORT, register, value);
    }
    for (character, glyph) in text_mode::FONT
        .as_chunks::<{ text_mode::CHARACTER_HEIGHT as usize }>()
        .0
        .iter()
        .enumerate()
    {
        let mut slot = [0; 32];
        slot[..glyph.len()].copy_from_slice(glyph);
        memory
            .write(0xa0000 + character as u32 * 32, &slot)
            .expect("text font");
    }
}
