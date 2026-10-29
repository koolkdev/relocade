//! Register values for the supported 80-column, eight-dot color text mode.
//! Device defaults use these tables. BIOS mode setting currently restores the
//! CRTC table and clock controls; complete bank restoration belongs to firmware.

use super::{attribute, crtc, graphics, sequencer};

pub const CHARACTER_HEIGHT: u16 = 8;
/// Bootstrap CP437 font. Firmware can upload it through the ordinary VGA aperture.
pub const FONT: &[u8; 256 * CHARACTER_HEIGHT as usize] =
    include_bytes!("../../assets/modern-dos-8x8.bin");
/// Color I/O, video RAM enabled, 25.175 MHz oscillator.
pub const MISC_OUTPUT: u8 = 0x43;
/// Eight dots per character, undivided clock, screen on.
pub const CLOCKING_MODE: u8 = 0x01;
/// Retrace ends at line 414; timing-register protection is enabled.
pub const VERTICAL_RETRACE_END: u8 = 0xae;

pub const SEQUENCER_REGISTERS: &[(u8, u8)] = &[
    (sequencer::RESET, sequencer::RESET_RELEASED),
    (sequencer::CLOCKING_MODE, CLOCKING_MODE),
    (sequencer::MAP_MASK, 3), // Character and attribute planes.
    (sequencer::CHARACTER_MAP_SELECT, 0),
    (sequencer::MEMORY_MODE, 2), // Extended memory, odd/even access.
];

pub const GRAPHICS_REGISTERS: &[(u8, u8)] = &[
    (graphics::SET_RESET, 0),
    (graphics::ENABLE_SET_RESET, 0),
    (graphics::COLOR_COMPARE, 0),
    (graphics::DATA_ROTATE, 0),
    (graphics::READ_MAP_SELECT, 0),
    (graphics::MODE, 0x10),          // Odd/even CPU reads, write mode zero.
    (graphics::MISCELLANEOUS, 0x0e), // Text addressing at B8000.
    (graphics::COLOR_DONT_CARE, 0x0f),
    (graphics::BIT_MASK, 0xff),
];

/// EGA palette addresses for the sixteen PC text attributes.
pub const ATTRIBUTE_PALETTE: [u8; 16] = [
    0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
];

pub const ATTRIBUTE_REGISTERS: &[(u8, u8)] = &[
    (attribute::MODE_CONTROL, 0x0c), // Blink and line-graphics extension.
    (attribute::OVERSCAN_COLOR, 0),
    (attribute::COLOR_PLANE_ENABLE, 0x0f),
    (attribute::HORIZONTAL_PANNING, 0),
    (attribute::COLOR_SELECT, 0),
];

/// 100 eight-dot character clocks per line and 449 lines per frame, with
/// 640 × 400 active dots. The fixed eight-row font is double-scanned.
/// Entries without a decoded timing comment are retained for guest readback;
/// their per-pixel blanking/retrace effects are not modeled yet.
/// The final entry restores protection after programming the timing registers.
pub const CRTC_REGISTERS: &[(u8, u8)] = &[
    (crtc::HORIZONTAL_TOTAL, 0x5f),       // 95 + 5 = 100 character clocks.
    (crtc::HORIZONTAL_DISPLAY_END, 0x4f), // 79 + 1 = 80 visible character clocks.
    (crtc::HORIZONTAL_BLANK_START, 0x50),
    (crtc::HORIZONTAL_BLANK_END, 0x82),
    (crtc::HORIZONTAL_RETRACE_START, 0x54),
    (crtc::HORIZONTAL_RETRACE_END, 0x80),
    (crtc::VERTICAL_TOTAL, 0xbf), // Overflow supplies bit 8: 447 + 2 = 449 lines.
    (crtc::OVERFLOW, 0x1f),
    (crtc::PRESET_ROW_SCAN, 0),
    (crtc::MAXIMUM_SCAN_LINE, 0xc0 | (CHARACTER_HEIGHT as u8 - 1)),
    (crtc::VERTICAL_RETRACE_START, 0x9c), // Overflow supplies bit 8: line 412.
    (crtc::VERTICAL_DISPLAY_END, 0x8f),   // Overflow supplies bit 8: 399 + 1 = 400 lines.
    (crtc::OFFSET, 40),                   // Two character addresses per offset unit.
    (crtc::UNDERLINE_LOCATION, 0x1f),     // Below the eight-row font.
    (crtc::VERTICAL_BLANK_START, 0x96),
    (crtc::VERTICAL_BLANK_END, 0xb9),
    (crtc::MODE_CONTROL, 0xa3), // Sync enabled, one scanline per vertical count.
    (crtc::LINE_COMPARE, 0xff), // Overflow bits select line 1023, beyond the raster.
    (crtc::VERTICAL_RETRACE_END, VERTICAL_RETRACE_END),
];
