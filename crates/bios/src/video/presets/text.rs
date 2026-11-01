//! Eight-dot, 80-column text profiles. All three start with 25 character rows.
use super::{
    crtc::{CrtcPreset, VERTICAL_350},
    Preset,
};
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
    crtc: CrtcPreset::COLOR_TEXT_200,
    attribute: text_mode::ATTRIBUTE_REGISTERS,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};

// 640×350 active dots, 100 character clocks per line, 449 lines per frame.
// Sync polarity and vertical intervals follow the VGA 350-line raster.
pub(in crate::video) const TEXT_350: Preset = Preset {
    character_height: 14,
    miscellaneous: 0xa3,
    crtc: TEXT_200
        .crtc
        .with(VERTICAL_350)
        .with(&[(crtc::MAXIMUM_SCAN_LINE, 0x4d)]),
    ..TEXT_200
};

// 640×400 active dots with 16-high glyphs and no double scanning.
pub(in crate::video) const TEXT_400: Preset = Preset {
    character_height: 16,
    crtc: TEXT_200.crtc.with(&[(crtc::MAXIMUM_SCAN_LINE, 0x4f)]),
    ..TEXT_200
};
