//! Eight-dot, 80-column text profiles. All three start with 25 character rows.
use super::Preset;
use relocade_vga::registers::{crtc, text_mode};

// Retain the existing 8-high, double-scanned startup profile.
pub(in crate::video) const TEXT_200: Preset = Preset {
    columns: 80,
    character_rows: 25,
    character_height: 8,
    page_bytes: 0x1000,
    miscellaneous: text_mode::MISC_OUTPUT,
    sequencer: text_mode::SEQUENCER_REGISTERS,
    graphics: text_mode::GRAPHICS_REGISTERS,
    crtc: text_mode::CRTC_REGISTERS,
    attribute: text_mode::ATTRIBUTE_REGISTERS,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};

// 640×350 active dots, 100 character clocks per line, 449 lines per frame.
// Sync polarity and vertical intervals follow the VGA 350-line raster.
pub(in crate::video) const TEXT_350: Preset = Preset {
    character_height: 14,
    miscellaneous: 0xa3,
    crtc: &[
        (crtc::HORIZONTAL_TOTAL, 0x5f),
        (crtc::HORIZONTAL_DISPLAY_END, 0x4f),
        (crtc::HORIZONTAL_BLANK_START, 0x50),
        (crtc::HORIZONTAL_BLANK_END, 0x82),
        (crtc::HORIZONTAL_RETRACE_START, 0x54),
        (crtc::HORIZONTAL_RETRACE_END, 0x80),
        (crtc::VERTICAL_TOTAL, 0xbf),
        (crtc::OVERFLOW, 0x1f),
        (crtc::PRESET_ROW_SCAN, 0),
        (crtc::MAXIMUM_SCAN_LINE, 0x4d),
        (crtc::VERTICAL_RETRACE_START, 0x83),
        (crtc::VERTICAL_DISPLAY_END, 0x5d),
        (crtc::OFFSET, 40),
        (crtc::UNDERLINE_LOCATION, 0x1f),
        (crtc::VERTICAL_BLANK_START, 0x63),
        (crtc::VERTICAL_BLANK_END, 0xba),
        (crtc::MODE_CONTROL, 0xa3),
        (crtc::LINE_COMPARE, 0xff),
        (crtc::VERTICAL_RETRACE_END, 0x85),
    ],
    ..TEXT_200
};

// 640×400 active dots with 16-high glyphs and no double scanning.
pub(in crate::video) const TEXT_400: Preset = Preset {
    character_height: 16,
    crtc: &[
        (crtc::HORIZONTAL_TOTAL, 0x5f),
        (crtc::HORIZONTAL_DISPLAY_END, 0x4f),
        (crtc::HORIZONTAL_BLANK_START, 0x50),
        (crtc::HORIZONTAL_BLANK_END, 0x82),
        (crtc::HORIZONTAL_RETRACE_START, 0x54),
        (crtc::HORIZONTAL_RETRACE_END, 0x80),
        (crtc::VERTICAL_TOTAL, 0xbf),
        (crtc::OVERFLOW, 0x1f),
        (crtc::PRESET_ROW_SCAN, 0),
        (crtc::MAXIMUM_SCAN_LINE, 0x4f),
        (crtc::VERTICAL_RETRACE_START, 0x9c),
        (crtc::VERTICAL_DISPLAY_END, 0x8f),
        (crtc::OFFSET, 40),
        (crtc::UNDERLINE_LOCATION, 0x1f),
        (crtc::VERTICAL_BLANK_START, 0x96),
        (crtc::VERTICAL_BLANK_END, 0xb9),
        (crtc::MODE_CONTROL, 0xa3),
        (crtc::LINE_COMPARE, 0xff),
        (crtc::VERTICAL_RETRACE_END, text_mode::VERTICAL_RETRACE_END),
    ],
    ..TEXT_200
};
