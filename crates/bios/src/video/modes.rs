//! Firmware mode transitions through the same ports and MMIO used by guests.
use super::{
    crtc_port, fonts, graphics, palette, parameters::Parameters, presets, write_indexed,
    ACTIVE_PAGE, COLUMNS, CRTC_PORT, CURSORS, LAST_ROW, MODE, MODESET_CONTROL, PAGE_OFFSET,
    PAGE_SIZE, VIDEO_CONTROL, VIDEO_SWITCHES,
};
use crate::screen;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};
use relocade_vga::{
    registers::{sequencer, SEQUENCER_INDEX_PORT},
    TEXT_MEMORY_BYTES, TEXT_MEMORY_START,
};

// BIOS mode numbers accepted by INT 10h/AH=00h.
// VGA gives each legacy burst-disabled/color text pair identical color output.
pub(super) const TEXT_40X25: u8 = 0x00;
pub(super) const COLOR_TEXT_40X25: u8 = 0x01;
pub(super) const TEXT_80X25: u8 = 0x02;
pub(crate) const COLOR_TEXT_80X25: u8 = 0x03;
pub(super) const GRAPHICS_320X200_4: u8 = 0x04;
// VGA has identical output for the original CGA color/burst-disabled pair.
pub(super) const GRAPHICS_320X200_4_ALT: u8 = 0x05;
pub(super) const GRAPHICS_640X200_2: u8 = 0x06;
pub(super) const MONO_TEXT_80X25: u8 = 0x07;
pub(super) const GRAPHICS_320X200_16: u8 = 0x0d;
pub(super) const GRAPHICS_640X200_16: u8 = 0x0e;
pub(super) const GRAPHICS_640X350_MONO: u8 = 0x0f;
pub(super) const GRAPHICS_640X350_16: u8 = 0x10;
pub(crate) const GRAPHICS_640X480_2: u8 = 0x11;
pub(super) const GRAPHICS_640X480_16: u8 = 0x12;
pub(super) const GRAPHICS_320X200_256: u8 = 0x13;

/// BIOS text modes share the same planes, exposed through different CPU apertures.
pub(crate) fn text_memory_start(mode: u8) -> Option<u32> {
    match mode {
        TEXT_40X25..=COLOR_TEXT_80X25 => Some(TEXT_MEMORY_START),
        MONO_TEXT_80X25 => Some(0xb0000),
        _ => None,
    }
}

/// The mode (including its memory-preservation bit), columns and active page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeInfo {
    pub mode: u8,
    pub columns: u8,
    pub page: u8,
}

/// Returns the state reported by INT 10h/AH=0Fh.
#[interrupt(context(memory), returns { mode => { al = mode.mode, ah = mode.columns, bh = mode.page } })]
pub fn mode(memory: &MemoryView<'_>) -> ModeInfo {
    let mode = memory.read_u8(MODE).expect("video mode");
    let preserved = memory.read_u8(VIDEO_CONTROL).expect("video control") & 0x80;
    ModeInfo {
        mode: mode | preserved,
        columns: memory.read_u16(COLUMNS).expect("video columns") as u8,
        page: screen::active_page(memory),
    }
}

/// Program standard VGA. A selected card's firmware prepares its interface first.
pub fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
    Some(ModeSet::prepare(memory, request)?.apply(memory, io))
}

/// Validated legacy programming, captured before the card changes CPU mappings.
pub(super) struct ModeSet {
    request: u8,
    preset: &'static presets::Preset,
    parameters: Parameters,
}

impl ModeSet {
    pub(super) fn prepare(memory: &MemoryView<'_>, request: u8) -> Option<Self> {
        let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
        let record = presets::record_index(request & 0x7f, control)?;
        let preset = presets::TABLE[record].expect("supported BIOS mode");
        let parameters = Parameters::read(memory, record)?;
        Some(Self {
            request,
            preset,
            parameters,
        })
    }

    pub(super) fn apply(self, memory: &mut MemoryView<'_>, io: &IoBus) -> u8 {
        let Self {
            request,
            preset,
            parameters,
        } = self;
        let mode = request & 0x7f;
        let text_memory = text_memory_start(mode);
        // Restore routing and write masks before any clear. Font loading temporarily
        // selects plane two, then the mode record establishes guest memory access.
        write_indexed(
            io,
            SEQUENCER_INDEX_PORT,
            sequencer::RESET,
            sequencer::SYNCHRONOUS_RESET,
        );
        fonts::set_mode(memory, io, preset.character_height, text_memory.is_some());
        let palette = palette::PaletteLoad::for_mode_set(memory);
        parameters.program(memory, io, palette);
        preset.load_palette(io, palette);

        complete_mode(memory, io, request, &parameters, |memory| {
            if let Some(base) = text_memory {
                let blank = [b' ', 7].repeat(2048);
                for offset in (0..TEXT_MEMORY_BYTES).step_by(blank.len()) {
                    memory.write(base + offset, &blank).expect("text page");
                }
            } else {
                graphics::clear(memory);
            }
        });
        match mode {
            TEXT_40X25..=GRAPHICS_320X200_4_ALT | MONO_TEXT_80X25 => 0x30,
            GRAPHICS_640X200_2 => 0x3f,
            _ => 0x20,
        }
    }
}

/// Publish the programmed geometry, clear according to the card's memory policy,
/// and apply guest font/palette overrides before releasing the sequencer.
pub(super) fn complete_mode(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    request: u8,
    parameters: &Parameters,
    clear: impl FnOnce(&mut MemoryView<'_>),
) {
    let mode = request & 0x7f;
    memory.write_u8(MODE, mode).expect("video mode");
    palette::initialize_compatibility(memory, mode);
    memory
        .write_u16(COLUMNS, u16::from(parameters.columns))
        .expect("video columns");
    memory
        .write_u16(PAGE_SIZE, parameters.page_bytes)
        .expect("video page size");
    memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    memory.write_u8(ACTIVE_PAGE, 0).expect("active video page");
    screen::set_native_cursor_shape(memory, io, parameters.cursor_shape());
    memory
        .write_u16(CRTC_PORT, crtc_port(io))
        .expect("CRTC port");
    memory
        .write(
            LAST_ROW,
            &[parameters.last_row, parameters.character_height, 0],
        )
        .expect("video font geometry");
    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    memory
        .write_u8(VIDEO_CONTROL, (control & 0x7f) | (request & 0x80))
        .expect("video control");
    if request & 0x80 == 0 {
        clear(memory);
    }
    fonts::apply_guest_tables(memory, io);
    palette::apply_guest_tables(memory, io);
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::RESET_RELEASED,
    );
}

/// AH=12h/BL=30h selects a profile for a later mode set; it does not touch VGA.
/// Returns whether the profile is supported.
#[relocade_machine::real_mode_interrupt::interrupt(context(memory), args(selection = al),
    returns { true => { al = 0x12 }, false => crate::interrupts::unsupported }
)]
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
