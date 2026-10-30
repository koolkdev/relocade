//! Output geometry needed to allocate an image from captured display state.
use crate::{
    registers::{crtc, sequencer},
    DisplaySnapshot,
};

/// Image dimensions and the relationship between output rows and VGA scanlines.
/// Pixel interpretation belongs to the renderer and uses the captured registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGeometry {
    pub size: [u32; 2],
    /// Physical scanlines represented by each image row. Identical doubled rows
    /// share storage unless a line-compare split falls between the pair.
    pub scanline_step: u32,
}

impl DisplaySnapshot {
    /// Output geometry for the supported text layout. Graphics layouts will
    /// extend this calculation alongside their shader decoding.
    pub fn geometry(&self) -> DisplayGeometry {
        let c = &self.registers.crtc.values;
        let clocking_mode = self.registers.sequencer.values[sequencer::CLOCKING_MODE as usize];
        let glyph_width = if clocking_mode & 1 != 0 { 8 } else { 9 };
        let columns = u32::from(c[crtc::HORIZONTAL_DISPLAY_END as usize]) + 1;
        let last_line = u32::from(c[crtc::VERTICAL_DISPLAY_END as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 2) << 7)
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x40) << 3);
        let line_compare = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3);
        let doubled = c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x80 != 0;
        let split_pair = line_compare < last_line && line_compare % 2 == 0;
        let scanline_step = if doubled && !split_pair { 2 } else { 1 };
        DisplayGeometry {
            size: [
                columns * glyph_width,
                (last_line + 1).div_ceil(scanline_step),
            ],
            scanline_step,
        }
    }
}

impl DisplayGeometry {
    /// Nominal 4:3 VGA display proportions. Physical monitor reconstruction is
    /// separate from the frame-snapshot approximation.
    pub fn pixel_aspect_ratio(&self) -> (u32, u32) {
        let (width, height) = (self.size[1] * 4, self.size[0] * 3);
        let (mut a, mut b) = (width, height);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        (width / a, height / a)
    }
}
