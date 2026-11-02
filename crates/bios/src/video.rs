//! BIOS video services callable from Rust and through INT 10h.
//!
//! Direct calls take memory, I/O and ordinary values; they never access CPU
//! registers. Invalid mode/page requests return None. The interrupt adapter maps
//! that outcome to AH=86h/CF and writes declared successful results.
//! BIOS bookkeeping never selects host rendering.

mod adapter;
mod characters;
mod cursor;
mod fonts;
pub(super) mod graphics;
mod information;
mod interrupt;
mod modes;
mod palette;
mod presets;
mod scroll;
mod state;
mod strings;

pub use characters::{
    read_character, teletype, write_characters, write_characters_preserving_attributes,
    write_characters_with_attribute, CharacterRun, TextCell,
};
pub use cursor::{cursor, select_page, set_cursor, set_cursor_shape, CursorState};
pub(super) use fonts::Glyph;
pub use graphics::pixel::{read_pixel, write_pixel};
pub(crate) use interrupt::dispatch_entry;
pub use modes::{mode, set_mode, set_screen_output, ModeInfo};
pub(super) use modes::{text_memory_start, COLOR_TEXT_80X25, GRAPHICS_640X480_2};
pub use scroll::{scroll, scroll_down, scroll_up};

pub use crate::screen::{ScrollDirection, Window};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{
    COLOR_INPUT_STATUS_1_PORT, CRTC_INDEX_PORT, MISC_OUTPUT_READ_PORT, MONO_CRTC_INDEX_PORT,
    MONO_INPUT_STATUS_1_PORT,
};

// Firmware video data in segment 0040h.
pub(super) const MODE: u32 = 0x449;
pub(super) const COLUMNS: u32 = 0x44a;
pub(super) const PAGE_SIZE: u32 = 0x44c;
pub(super) const PAGE_OFFSET: u32 = 0x44e;
pub(super) const CURSORS: u32 = 0x450;
pub(super) const CURSOR_SHAPE: u32 = 0x460;
pub(super) const ACTIVE_PAGE: u32 = 0x462;
const CRTC_PORT: u32 = 0x463;
pub(super) const LAST_ROW: u32 = 0x484;
pub(super) const CHARACTER_HEIGHT: u32 = 0x485;
pub(super) const VIDEO_CONTROL: u32 = 0x487;
const VIDEO_SWITCHES: u32 = 0x488;
const MODESET_CONTROL: u32 = 0x489;

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    adapter::initialize(memory);
    fonts::initialize(memory);
    information::initialize(memory);
    // Preserve the existing doubled 8-high startup.
    assert!(
        modes::select_scanlines(memory, 0),
        "initial scanline profile"
    );
    set_mode(memory, io, COLOR_TEXT_80X25).expect("initial text mode");
}

fn write_indexed(io: &IoBus, port: u16, register: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([register, value])));
}

fn read_indexed(io: &IoBus, port: u16, register: u8) -> u8 {
    io.write_port(port, 1, u32::from(register));
    io.read_port(port + 1, 1) as u8
}

pub(super) fn write_crtc_word(io: &IoBus, register: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    let port = crtc_port(io);
    write_indexed(io, port, register, high);
    write_indexed(io, port, register + 1, low);
}

fn crtc_port(io: &IoBus) -> u16 {
    if io.read_port(MISC_OUTPUT_READ_PORT, 1) & 1 != 0 {
        CRTC_INDEX_PORT
    } else {
        MONO_CRTC_INDEX_PORT
    }
}

fn input_status_1_port(io: &IoBus) -> u16 {
    if crtc_port(io) == CRTC_INDEX_PORT {
        COLOR_INPUT_STATUS_1_PORT
    } else {
        MONO_INPUT_STATUS_1_PORT
    }
}
