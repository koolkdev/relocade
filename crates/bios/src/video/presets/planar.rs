//! Four-plane BIOS modes share CPU routing and the serial shifter.
use super::{
    crtc::{CrtcPreset, HORIZONTAL_40, VERTICAL_350, VERTICAL_480},
    palette, Preset,
};
use relocade_vga::registers::{attribute, crtc, graphics, sequencer, text_mode};

const fn graphics_registers(compare_planes: u8) -> [(u8, u8); 9] {
    [
        (graphics::SET_RESET, 0),
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::COLOR_COMPARE, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::READ_MAP_SELECT, 0),
        (graphics::MODE, 0),          // Serial shift, CPU read/write mode zero.
        (graphics::MISCELLANEOUS, 5), // A0000–AFFFF, independent planes.
        (graphics::COLOR_DONT_CARE, compare_planes),
        (graphics::BIT_MASK, 255),
    ]
}
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
    graphics: &graphics_registers(15),
    crtc: CrtcPreset::COLOR_TEXT_200.with(HORIZONTAL_40).with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0xc0),
        (crtc::OFFSET, 20), // Forty plane bytes per row.
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::MODE_CONTROL, 0xe3), // Byte addresses, no row-scan substitution.
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    attribute: ATTRIBUTE,
    attribute_palette: [
        0, 1, 2, 3, 4, 5, 6, 7, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
    ],
    palette: &palette::RGBI,
};

// Full-rate dot clock with the same doubled raster and RGBI palette as mode 0Dh.
pub(in crate::video) const PLANAR_640X200: Preset = Preset {
    columns: 80,
    page_bytes: 0x4000,
    sequencer: PLANAR_640X480.sequencer,
    crtc: CrtcPreset::COLOR_TEXT_200.with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0xc0),
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::MODE_CONTROL, 0xe3),
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    ..PLANAR_320X200
};

// Single-scan EGA-compatible geometry with two 32-KiB pages per plane.
pub(in crate::video) const PLANAR_640X350: Preset = Preset {
    character_rows: 25,
    character_height: 14,
    page_bytes: 0x8000,
    miscellaneous: 0xa3,
    crtc: PLANAR_640X480.crtc.with(VERTICAL_350),
    ..PLANAR_640X480
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
    graphics: &graphics_registers(15),
    crtc: CrtcPreset::COLOR_TEXT_200.with(VERTICAL_480).with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0x40),
        (crtc::UNDERLINE_LOCATION, 0),
        (crtc::MODE_CONTROL, 0xe3),
    ]),
    attribute: ATTRIBUTE,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};

// Mode 11h uses the same four-plane access as mode 12h. Its palette maps
// every nonzero color to white; read-mode-one comparisons use only plane zero.
pub(in crate::video) const MONO_640X480: Preset = Preset {
    graphics: &graphics_registers(1),
    crtc: PLANAR_640X480.crtc.with(&[(crtc::MODE_CONTROL, 0xc3)]),
    attribute_palette: [
        0, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    ],
    ..PLANAR_640X480
};
