//! BIOS 320×200, 256-color register and palette defaults.
use super::{crtc::CrtcPreset, palette, Preset};
use relocade_vga::registers::{attribute, crtc, graphics, sequencer};

// 25.175 MHz, 640 dots by 400 scanlines; the shifter consumes two dots per
// eight-bit pixel and CRTC row scan repeats each 320-byte memory row twice.
pub(in crate::video) const INDEXED256: Preset = Preset {
    columns: 40,
    character_rows: 25,
    character_height: 8,
    page_bytes: 64000,
    miscellaneous: 0x63,
    sequencer: &[
        (sequencer::CLOCKING_MODE, 1),
        (sequencer::MAP_MASK, 15),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 14), // Extended memory, chain-4 CPU access.
    ],
    graphics: &[
        (graphics::SET_RESET, 0),
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::COLOR_COMPARE, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::READ_MAP_SELECT, 0),
        (graphics::MODE, 0x40), // 256-color shift, CPU read/write mode zero.
        (graphics::MISCELLANEOUS, 5), // A0000–AFFFF graphics aperture.
        (graphics::COLOR_DONT_CARE, 15),
        (graphics::BIT_MASK, 255),
    ],
    crtc: CrtcPreset::COLOR_TEXT_200.with(&[
        (crtc::MAXIMUM_SCAN_LINE, 0x41),
        (crtc::UNDERLINE_LOCATION, 0x40), // Doubleword addressing.
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    attribute: &[
        (attribute::MODE_CONTROL, 0x41), // Graphics, eight-bit color.
        (attribute::OVERSCAN_COLOR, 0),
        (attribute::COLOR_PLANE_ENABLE, 15),
        (attribute::HORIZONTAL_PANNING, 0),
        (attribute::COLOR_SELECT, 0),
    ],
    attribute_palette: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    palette: &palette::INDEXED256,
};
