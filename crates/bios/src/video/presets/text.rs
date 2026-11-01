//! Eight-dot color text profiles. Each width starts with 25 character rows.
use super::{
    crtc::{CrtcPreset, HORIZONTAL_40, VERTICAL_350},
    Preset,
};
use relocade_vga::registers::{crtc, sequencer, text_mode};

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

// Forty-column counterpart of the 200-line profile.
pub(in crate::video) const TEXT_40_200: Preset = Preset {
    columns: 40,
    page_bytes: 0x0800,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 9), // Half-rate clock for forty eight-dot cells.
        (sequencer::MAP_MASK, 3),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 2),
    ],
    crtc: TEXT_200
        .crtc
        .with(HORIZONTAL_40)
        .with(&[(crtc::OFFSET, 20)]),
    ..TEXT_200
};

// Forty-column counterpart of the 350-line profile.
pub(in crate::video) const TEXT_40_350: Preset = Preset {
    columns: 40,
    page_bytes: 0x0800,
    sequencer: TEXT_40_200.sequencer,
    crtc: TEXT_350
        .crtc
        .with(HORIZONTAL_40)
        .with(&[(crtc::OFFSET, 20)]),
    ..TEXT_350
};

// Forty-column counterpart of the 400-line profile.
pub(in crate::video) const TEXT_40_400: Preset = Preset {
    columns: 40,
    page_bytes: 0x0800,
    sequencer: TEXT_40_200.sequencer,
    crtc: TEXT_400
        .crtc
        .with(HORIZONTAL_40)
        .with(&[(crtc::OFFSET, 20)]),
    ..TEXT_400
};
