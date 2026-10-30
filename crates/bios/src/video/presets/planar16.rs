//! Four-plane BIOS modes share CPU routing and the serial shifter.
use super::{palette, Preset};
use relocade_vga::registers::{attribute, crtc, graphics, sequencer, text_mode};

const GRAPHICS: &[(u8, u8)] = &[
    (graphics::SET_RESET, 0),
    (graphics::ENABLE_SET_RESET, 0),
    (graphics::COLOR_COMPARE, 0),
    (graphics::DATA_ROTATE, 0),
    (graphics::READ_MAP_SELECT, 0),
    (graphics::MODE, 0),          // Serial shift, CPU read/write mode zero.
    (graphics::MISCELLANEOUS, 5), // A0000–AFFFF, independent planes.
    (graphics::COLOR_DONT_CARE, 15),
    (graphics::BIT_MASK, 255),
];
const ATTRIBUTE: &[(u8, u8)] = &[
    (attribute::MODE_CONTROL, 1),
    (attribute::OVERSCAN_COLOR, 0),
    (attribute::COLOR_PLANE_ENABLE, 15),
    (attribute::HORIZONTAL_PANNING, 0),
    (attribute::COLOR_SELECT, 0),
];

// Half-rate dot clock and double scanning: 320 pixels by 200 memory rows.
pub(in crate::video) const PLANAR_320X200: Preset = Preset {
    columns: 40,
    character_rows: 25,
    character_height: 8,
    page_bytes: 0x2000,
    miscellaneous: 0x63,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 9),
        (sequencer::MAP_MASK, 15),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 6),
    ],
    graphics: GRAPHICS,
    crtc: &[
        (crtc::HORIZONTAL_TOTAL, 0x2d),
        (crtc::HORIZONTAL_DISPLAY_END, 0x27),
        (crtc::HORIZONTAL_BLANK_START, 0x28),
        (crtc::HORIZONTAL_BLANK_END, 0x90),
        (crtc::HORIZONTAL_RETRACE_START, 0x2b),
        (crtc::HORIZONTAL_RETRACE_END, 0x80),
        (crtc::VERTICAL_TOTAL, 0xbf),
        (crtc::OVERFLOW, 0x1f),
        (crtc::PRESET_ROW_SCAN, 0),
        (crtc::MAXIMUM_SCAN_LINE, 0xc0),
        (crtc::VERTICAL_RETRACE_START, 0x9c),
        (crtc::VERTICAL_DISPLAY_END, 0x8f),
        (crtc::OFFSET, 20), // Forty plane bytes per row.
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::VERTICAL_BLANK_START, 0x96),
        (crtc::VERTICAL_BLANK_END, 0xb9),
        (crtc::MODE_CONTROL, 0xe3), // Byte addresses, no row-scan substitution.
        (crtc::LINE_COMPARE, 0xff),
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ],
    attribute: ATTRIBUTE,
    attribute_palette: [
        0, 1, 2, 3, 4, 5, 6, 7, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
    ],
    palette: &palette::RGBI,
};

// Single scan, 640×480. The 80×30 character grid is BIOS bookkeeping only.
pub(in crate::video) const PLANAR_640X480: Preset = Preset {
    columns: 80,
    character_rows: 30,
    character_height: 16,
    page_bytes: 0xa000,
    miscellaneous: 0xe3,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 1),
        (sequencer::MAP_MASK, 15),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 6),
    ],
    graphics: GRAPHICS,
    crtc: &[
        (crtc::HORIZONTAL_TOTAL, 0x5f),
        (crtc::HORIZONTAL_DISPLAY_END, 0x4f),
        (crtc::HORIZONTAL_BLANK_START, 0x50),
        (crtc::HORIZONTAL_BLANK_END, 0x82),
        (crtc::HORIZONTAL_RETRACE_START, 0x54),
        (crtc::HORIZONTAL_RETRACE_END, 0x80),
        (crtc::VERTICAL_TOTAL, 0x0b),
        (crtc::OVERFLOW, 0x3e),
        (crtc::PRESET_ROW_SCAN, 0),
        (crtc::MAXIMUM_SCAN_LINE, 0x40),
        (crtc::VERTICAL_RETRACE_START, 0xea),
        (crtc::VERTICAL_DISPLAY_END, 0xdf),
        (crtc::OFFSET, 40), // Eighty plane bytes per row.
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::VERTICAL_BLANK_START, 0xe7),
        (crtc::VERTICAL_BLANK_END, 4),
        (crtc::MODE_CONTROL, 0xe3),
        (crtc::LINE_COMPARE, 0xff),
        (crtc::VERTICAL_RETRACE_END, 0x8c),
    ],
    attribute: ATTRIBUTE,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};
