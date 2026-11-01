//! VGA's banked B8000 compatibility modes; no separate CGA device is involved.
use super::{
    crtc::{CrtcPreset, HORIZONTAL_40},
    palette, Preset,
};
use relocade_vga::registers::{attribute, crtc, graphics, sequencer};

pub(in crate::video) const CGA_320X200: Preset = Preset {
    columns: 40,
    character_rows: 25,
    character_height: 8,
    page_bytes: 0x4000,
    miscellaneous: 0x63,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 9),
        (sequencer::MAP_MASK, 3),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 2), // Odd/even writes to maps zero and one.
    ],
    graphics: &[
        (graphics::SET_RESET, 0),
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::COLOR_COMPARE, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::READ_MAP_SELECT, 0),
        (graphics::MODE, 0x30), // Shift interleave and odd/even reads.
        (graphics::MISCELLANEOUS, 0x0f), // B8000–BFFFF with odd/even address chaining.
        (graphics::COLOR_DONT_CARE, 15),
        (graphics::BIT_MASK, 255),
    ],
    crtc: CrtcPreset::COLOR_TEXT_200.with(HORIZONTAL_40).with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0xc1), // Two bank rows, each double scanned.
        (crtc::OFFSET, 20),
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::MODE_CONTROL, 0xa2), // Word addresses; RS0 selects the 8-KiB bank.
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    attribute: &[
        (attribute::MODE_CONTROL, 1),
        (attribute::OVERSCAN_COLOR, 0),
        (attribute::COLOR_PLANE_ENABLE, 3),
        (attribute::HORIZONTAL_PANNING, 0),
        (attribute::COLOR_SELECT, 0),
    ],
    attribute_palette: [
        0, 0x13, 0x15, 0x17, 2, 4, 6, 7, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
    ],
    palette: &palette::RGBI,
};

pub(in crate::video) const CGA_640X200: Preset = Preset {
    columns: 80,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 1),
        (sequencer::MAP_MASK, 1),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 6), // Every byte is in plane zero.
    ],
    graphics: &[
        (graphics::SET_RESET, 0),
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::COLOR_COMPARE, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::READ_MAP_SELECT, 0),
        (graphics::MODE, 0),             // Serial shifter.
        (graphics::MISCELLANEOUS, 0x0d), // B8000–BFFFF, unchained.
        (graphics::COLOR_DONT_CARE, 15),
        (graphics::BIT_MASK, 255),
    ],
    crtc: CrtcPreset::COLOR_TEXT_200.with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0xc1),
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::MODE_CONTROL, 0xc2), // Byte addresses; RS0 selects the 8-KiB bank.
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    attribute: &[
        (attribute::MODE_CONTROL, 1),
        (attribute::OVERSCAN_COLOR, 0),
        (attribute::COLOR_PLANE_ENABLE, 1),
        (attribute::HORIZONTAL_PANNING, 0),
        (attribute::COLOR_SELECT, 0),
    ],
    attribute_palette: [
        0, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17, 0x17,
    ],
    ..CGA_320X200
};
