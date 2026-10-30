//! Register presets for BIOS modes. Rendering interprets the resulting hardware state.
use super::{palette, write_indexed};
use relocade_machine::IoBus;
use relocade_vga::registers::{
    attribute, crtc, graphics, sequencer, text_mode, ATTRIBUTE_ADDRESS_PORT,
    COLOR_INPUT_STATUS_1_PORT, CRTC_INDEX_PORT, DAC_DATA_PORT, DAC_MASK_PORT,
    DAC_WRITE_ADDRESS_PORT, GRAPHICS_INDEX_PORT, MISC_OUTPUT_WRITE_PORT, SEQUENCER_INDEX_PORT,
};

pub(super) struct Preset {
    miscellaneous: u8,
    sequencer: &'static [(u8, u8)],
    graphics: &'static [(u8, u8)],
    crtc: &'static [(u8, u8)],
    attribute: &'static [(u8, u8)],
    attribute_palette: [u8; 16],
    palette: &'static [[u8; 3]; 256],
}

impl Preset {
    pub(super) fn program(&self, io: &IoBus) {
        io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(self.miscellaneous));
        for &(register, value) in self.sequencer {
            if register != sequencer::RESET {
                write_indexed(io, SEQUENCER_INDEX_PORT, register, value);
            }
        }
        for &(register, value) in self.graphics {
            write_indexed(io, GRAPHICS_INDEX_PORT, register, value);
        }
        write_indexed(io, CRTC_INDEX_PORT, crtc::VERTICAL_RETRACE_END, 0);
        for &(register, value) in self.crtc {
            write_indexed(io, CRTC_INDEX_PORT, register, value);
        }
        io.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
        for (index, value) in self.attribute_palette.into_iter().enumerate() {
            io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, index as u32);
            io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
        }
        for &(register, value) in self.attribute {
            io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(register));
            io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
        }
        io.write_port(
            ATTRIBUTE_ADDRESS_PORT,
            1,
            u32::from(attribute::PALETTE_ADDRESS_SOURCE),
        );
        io.write_port(DAC_MASK_PORT, 1, 0xff);
        io.write_port(DAC_WRITE_ADDRESS_PORT, 1, 0);
        for color in self.palette {
            for &component in color {
                io.write_port(DAC_DATA_PORT, 1, u32::from(component));
            }
        }
    }
}

pub(super) const TEXT: Preset = Preset {
    miscellaneous: text_mode::MISC_OUTPUT,
    sequencer: text_mode::SEQUENCER_REGISTERS,
    graphics: text_mode::GRAPHICS_REGISTERS,
    crtc: text_mode::CRTC_REGISTERS,
    attribute: text_mode::ATTRIBUTE_REGISTERS,
    attribute_palette: text_mode::ATTRIBUTE_PALETTE,
    palette: &text_mode::DAC_PALETTE,
};

// 25.175 MHz, 640 dots by 400 scanlines; the shifter consumes two dots per
// eight-bit pixel and CRTC row scan repeats each 320-byte memory row twice.
pub(super) const INDEXED256: Preset = Preset {
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
        (crtc::MAXIMUM_SCAN_LINE, 0x41),
        (crtc::VERTICAL_RETRACE_START, 0x9c),
        (crtc::VERTICAL_DISPLAY_END, 0x8f),
        (crtc::OFFSET, 40),               // 80 doublewords per memory row.
        (crtc::UNDERLINE_LOCATION, 0x40), // Doubleword addressing.
        (crtc::VERTICAL_BLANK_START, 0x96),
        (crtc::VERTICAL_BLANK_END, 0xb9),
        (crtc::MODE_CONTROL, 0xa3),
        (crtc::LINE_COMPARE, 0xff),
        (crtc::VERTICAL_RETRACE_END, 0x8e),
    ],
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
