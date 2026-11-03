//! Color text profiles. Each width starts with 25 character rows.
use super::{
    crtc::{CrtcPreset, HORIZONTAL_40, VERTICAL_350},
    Preset,
};
use relocade_vga::registers::{attribute, crtc, sequencer, text_mode};

// Legacy 200-line text repeats each glyph row on a 400-line raster.
pub(in crate::video) const TEXT_200: Preset = Preset {
    columns: 80,
    character_rows: 25,
    character_height: 8,
    page_bytes: 0x1000,
    miscellaneous: text_mode::MISC_OUTPUT,
    sequencer: text_mode::SEQUENCER_REGISTERS,
    graphics: text_mode::GRAPHICS_REGISTERS,
    crtc: CrtcPreset::COLOR_TEXT_200.with(&[(crtc::CURSOR_START, 6), (crtc::CURSOR_END, 7)]),
    attribute: text_mode::ATTRIBUTE_REGISTERS,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};

// 640×350 active dots, 100 character clocks per line, 449 lines per frame.
// Sync polarity and vertical intervals follow the VGA 350-line raster.
pub(in crate::video) const TEXT_350: Preset = Preset {
    character_height: 14,
    miscellaneous: 0xa3,
    crtc: TEXT_200.crtc.with(VERTICAL_350).with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0x4d),
        (crtc::CURSOR_START, 12),
        (crtc::CURSOR_END, 13),
    ]),
    ..TEXT_200
};

// IBM 400-line color text: 720×400 active dots, nine-dot cells at 28.322 MHz.
pub(in crate::video) const TEXT_400: Preset = Preset {
    character_height: 16,
    miscellaneous: 0x67,
    sequencer: &sequencer_registers(0),
    crtc: TEXT_200.crtc.with(&[
        (crtc::HORIZONTAL_RETRACE_START, 0x55),
        (crtc::HORIZONTAL_RETRACE_END, 0x81),
        (crtc::MAXIMUM_SCAN_LINE, 0x4f),
        (crtc::CURSOR_START, 13),
        (crtc::CURSOR_END, 14),
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    attribute: &[
        (attribute::MODE_CONTROL, 0x0c),
        (attribute::OVERSCAN_COLOR, 0),
        (attribute::COLOR_PLANE_ENABLE, 0x0f),
        (attribute::HORIZONTAL_PANNING, 8), // Unshifted nine-dot cells.
        (attribute::COLOR_SELECT, 0),
    ],
    ..TEXT_200
};

// Forty-column counterpart of the 200-line profile.
pub(in crate::video) const TEXT_40_200: Preset = Preset {
    columns: 40,
    page_bytes: 0x0800,
    sequencer: &sequencer_registers(9), // Half-rate clock for forty eight-dot cells.
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
    sequencer: &sequencer_registers(8), // Half-rate clock for forty nine-dot cells.
    crtc: TEXT_400.crtc.with(HORIZONTAL_40).with(&[
        (crtc::HORIZONTAL_RETRACE_END, 0xa0), // Blanking ends at character 48.
        (crtc::OFFSET, 20),
    ]),
    ..TEXT_400
};

// Text profiles share plane access and font selection; only character clocking varies.
pub(super) const fn sequencer_registers(clocking: u8) -> [(u8, u8); 4] {
    [
        (sequencer::CLOCKING_MODE, clocking),
        (sequencer::MAP_MASK, 3),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 2),
    ]
}
