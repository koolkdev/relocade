//! Register presets for BIOS modes. Rendering interprets the resulting hardware state.
mod cga;
mod crtc;
mod indexed256;
mod palette;
mod planar16;
mod text;

pub(super) use cga::{CGA_320X200, CGA_640X200};
pub(super) use indexed256::INDEXED256;
pub(super) use planar16::{PLANAR_320X200, PLANAR_640X200, PLANAR_640X350, PLANAR_640X480};
pub(super) use text::{TEXT_200, TEXT_350, TEXT_400};

use super::write_indexed;
use crtc::CrtcPreset;
use relocade_machine::IoBus;
use relocade_vga::registers::{
    attribute, sequencer, ATTRIBUTE_ADDRESS_PORT, COLOR_INPUT_STATUS_1_PORT, CRTC_INDEX_PORT,
    DAC_DATA_PORT, DAC_MASK_PORT, DAC_WRITE_ADDRESS_PORT, GRAPHICS_INDEX_PORT,
    MISC_OUTPUT_WRITE_PORT, SEQUENCER_INDEX_PORT,
};

pub(super) struct Preset {
    // BIOS data-area values describe the character grid even in graphics modes.
    pub(super) columns: u16,
    pub(super) character_rows: u8,
    pub(super) character_height: u8,
    pub(super) page_bytes: u16,
    miscellaneous: u8,
    sequencer: &'static [(u8, u8)],
    graphics: &'static [(u8, u8)],
    crtc: CrtcPreset,
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
        self.crtc.program(io, CRTC_INDEX_PORT);
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
