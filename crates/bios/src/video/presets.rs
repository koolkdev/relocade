//! Register presets for BIOS modes. Rendering interprets the resulting hardware state.
mod cga;
mod crtc;
mod indexed256;
mod monochrome;
mod palette;
mod planar;
mod text;

pub(super) use cga::{CGA_320X200, CGA_640X200};
pub(super) use indexed256::INDEXED256;
pub(super) use monochrome::{MONO_TEXT_350, MONO_TEXT_400};
pub(super) use planar::{
    MONO_640X350, MONO_640X480, PLANAR_320X200, PLANAR_640X200, PLANAR_640X350, PLANAR_640X480,
};
pub(super) use text::{TEXT_200, TEXT_350, TEXT_400, TEXT_40_200, TEXT_40_350, TEXT_40_400};

use super::{palette::PaletteLoad, parameters::Parameters};
use crtc::CrtcPreset;
use relocade_machine::IoBus;
use relocade_vga::registers::{crtc as crtc_registers, sequencer, DAC_MASK_PORT};

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
    pub(super) fn parameters(&self) -> Parameters {
        let mut sequencer = [0; 4];
        for &(register, value) in self.sequencer {
            if register != sequencer::RESET {
                sequencer[usize::from(register - 1)] = value;
            }
        }
        let mut graphics = [0; 9];
        for &(register, value) in self.graphics {
            graphics[usize::from(register)] = value;
        }
        let mut attribute = [0; 20];
        attribute[..16].copy_from_slice(&self.attribute_palette);
        for &(register, value) in self.attribute {
            if let Some(entry) = attribute.get_mut(usize::from(register)) {
                *entry = value;
            }
        }
        let mut crtc = self.crtc.registers();
        let text = attribute[0x10] & 1 == 0;
        crtc[crtc_registers::CURSOR_START as usize] =
            if text { self.character_height - 2 } else { 0 };
        crtc[crtc_registers::CURSOR_END as usize] =
            if text { self.character_height - 1 } else { 0 };
        Parameters {
            columns: self.columns as u8,
            last_row: self.character_rows - 1,
            character_height: self.character_height,
            page_bytes: self.page_bytes,
            sequencer,
            miscellaneous: self.miscellaneous,
            crtc,
            attribute,
            graphics,
        }
    }

    pub(super) fn load_palette(&self, io: &IoBus, palette: PaletteLoad) {
        if palette.loads_colors() {
            io.write_port(DAC_MASK_PORT, 1, 0xff);
            palette.write(io, 0, self.palette.iter().copied());
        }
    }
}

// IBM VGA record order. The 256-KiB adapter uses records 17/18 for modes 0Fh/10h;
// unused EGA-only records (8–12 and 15/16) remain zero.
pub(super) const TABLE: [Option<&Preset>; 29] = [
    Some(&TEXT_40_200),
    Some(&TEXT_40_200),
    Some(&TEXT_200),
    Some(&TEXT_200),
    Some(&CGA_320X200),
    Some(&CGA_320X200),
    Some(&CGA_640X200),
    Some(&MONO_TEXT_350),
    None,
    None,
    None,
    None,
    None,
    Some(&PLANAR_320X200),
    Some(&PLANAR_640X200),
    None,
    None,
    Some(&MONO_640X350),
    Some(&PLANAR_640X350),
    Some(&TEXT_40_350),
    Some(&TEXT_40_350),
    Some(&TEXT_350),
    Some(&TEXT_350),
    Some(&TEXT_40_400),
    Some(&TEXT_400),
    Some(&MONO_TEXT_400),
    Some(&MONO_640X480),
    Some(&PLANAR_640X480),
    Some(&INDEXED256),
];

/// Select the IBM table record for this mode and the BIOS text-scanline setting.
pub(super) fn record_index(mode: u8, mode_set_control: u8) -> Option<usize> {
    use super::modes::{
        COLOR_TEXT_40X25, COLOR_TEXT_80X25, GRAPHICS_320X200_16, GRAPHICS_320X200_256,
        GRAPHICS_320X200_4, GRAPHICS_320X200_4_ALT, GRAPHICS_640X200_16, GRAPHICS_640X200_2,
        GRAPHICS_640X350_16, GRAPHICS_640X350_MONO, GRAPHICS_640X480_16, GRAPHICS_640X480_2,
        MONO_TEXT_80X25, TEXT_40X25, TEXT_80X25,
    };

    let text_scanlines = match mode_set_control & 0x90 {
        0 => 350,
        0x10 => 400,
        _ => 200,
    };
    Some(match (mode, text_scanlines) {
        (TEXT_40X25, 200) => 0,
        (COLOR_TEXT_40X25, 200) => 1,
        (TEXT_80X25, 200) => 2,
        (COLOR_TEXT_80X25, 200) => 3,
        (TEXT_40X25, 350) => 19,
        (COLOR_TEXT_40X25, 350) => 20,
        (TEXT_80X25, 350) => 21,
        (COLOR_TEXT_80X25, 350) => 22,
        (TEXT_40X25 | COLOR_TEXT_40X25, 400) => 23,
        (TEXT_80X25 | COLOR_TEXT_80X25, 400) => 24,
        (MONO_TEXT_80X25, 400) => 25,
        (MONO_TEXT_80X25, _) => 7, // Mono has no 200-line profile; use 350 lines.
        // Graphics modes do not use the text-scanline setting.
        (GRAPHICS_320X200_4, _) => 4,
        (GRAPHICS_320X200_4_ALT, _) => 5,
        (GRAPHICS_640X200_2, _) => 6,
        (GRAPHICS_320X200_16, _) => 13,
        (GRAPHICS_640X200_16, _) => 14,
        (GRAPHICS_640X350_MONO, _) => 17,
        (GRAPHICS_640X350_16, _) => 18,
        (GRAPHICS_640X480_2, _) => 26,
        (GRAPHICS_640X480_16, _) => 27,
        (GRAPHICS_320X200_256, _) => 28,
        _ => return None,
    })
}
