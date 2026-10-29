//! Register values for the supported 80-column, eight-dot color text mode.
//! Device defaults and BIOS mode setting use this same preset.

use super::crtc;

pub const CHARACTER_HEIGHT: u16 = 8;
/// Color I/O, video RAM enabled, 25.175 MHz oscillator.
pub const MISC_OUTPUT: u8 = 0x43;
/// Eight dots per character, undivided clock, screen on.
pub const CLOCKING_MODE: u8 = 0x01;
/// Retrace ends at line 414; timing-register protection is enabled.
pub const VERTICAL_RETRACE_END: u8 = 0xae;

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
    (crtc::MAXIMUM_SCAN_LINE, 0xc0 | (CHARACTER_HEIGHT as u8 - 1)),
    (crtc::VERTICAL_RETRACE_START, 0x9c), // Overflow supplies bit 8: line 412.
    (crtc::VERTICAL_DISPLAY_END, 0x8f),   // Overflow supplies bit 8: 399 + 1 = 400 lines.
    (crtc::VERTICAL_BLANK_START, 0x96),
    (crtc::VERTICAL_BLANK_END, 0xb9),
    (crtc::MODE_CONTROL, 0xa3), // Sync enabled, one scanline per vertical count.
    (crtc::VERTICAL_RETRACE_END, VERTICAL_RETRACE_END),
];
