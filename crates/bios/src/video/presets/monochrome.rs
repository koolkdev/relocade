//! Nine-dot mode 07h profiles use the same text planes and glyph slots as color text.
use super::{palette, text, Preset};
use relocade_vga::registers::{attribute, crtc, graphics, sequencer};

pub(in crate::video) const MONO_TEXT_350: Preset = Preset {
    miscellaneous: 0xa6, // Monochrome ports, RAM enabled, 28.322 MHz oscillator.
    sequencer: &[
        (sequencer::CLOCKING_MODE, 0), // Nine dots per character.
        (sequencer::MAP_MASK, 3),
        (sequencer::CHARACTER_MAP_SELECT, 0),
        (sequencer::MEMORY_MODE, 2),
    ],
    graphics: &[
        (graphics::SET_RESET, 0),
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::COLOR_COMPARE, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::READ_MAP_SELECT, 0),
        (graphics::MODE, 0x10),
        (graphics::MISCELLANEOUS, 0x0a), // Odd/even text access at B0000.
        (graphics::COLOR_DONT_CARE, 15),
        (graphics::BIT_MASK, 255),
    ],
    crtc: text::TEXT_350.crtc.with(&[
        (crtc::HORIZONTAL_RETRACE_START, 0x55),
        (crtc::HORIZONTAL_RETRACE_END, 0x81),
        (crtc::UNDERLINE_LOCATION, 13),
    ]),
    attribute: &[
        (attribute::MODE_CONTROL, 0x0e), // Monochrome, blink, line graphics.
        (attribute::OVERSCAN_COLOR, 0),
        (attribute::COLOR_PLANE_ENABLE, 15),
        (attribute::HORIZONTAL_PANNING, 8), // Unshifted nine-dot text.
        (attribute::COLOR_SELECT, 0),
    ],
    attribute_palette: [
        0, 8, 8, 8, 8, 8, 8, 8, 0x10, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18,
    ],
    palette: &palette::MONOCHROME,
    ..text::TEXT_350
};

pub(in crate::video) const MONO_TEXT_400: Preset = Preset {
    character_height: 16,
    miscellaneous: 0x66,
    crtc: text::TEXT_400.crtc.with(&[
        (crtc::HORIZONTAL_RETRACE_START, 0x55),
        (crtc::HORIZONTAL_RETRACE_END, 0x81),
        (crtc::UNDERLINE_LOCATION, 15),
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ]),
    ..MONO_TEXT_350
};
