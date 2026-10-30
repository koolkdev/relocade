//! Output geometry needed to allocate an image from captured display state.
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    DisplaySnapshot,
};

/// Image dimensions and the relationship between output rows and VGA scanlines.
/// Pixel interpretation belongs to the renderer and uses the captured registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGeometry {
    pub size: [u32; 2],
    /// Physical scanlines represented by each image row. Identical repeated
    /// rows share storage unless panning or a split breaks their alignment.
    pub scanline_step: u32,
}

impl DisplaySnapshot {
    /// Output geometry for text and the eight-bit graphics layout.
    pub fn geometry(&self) -> DisplayGeometry {
        let c = &self.registers.crtc.values;
        let clocking_mode = self.registers.sequencer.values[sequencer::CLOCKING_MODE as usize];
        let glyph_width = if clocking_mode & 1 != 0 { 8 } else { 9 };
        let indexed256 = self.registers.attribute.values[attribute::MODE_CONTROL as usize] & 0x41
            == 0x41
            && self.registers.graphics.values[graphics::MODE as usize] & 0x40 != 0;
        let columns = u32::from(c[crtc::HORIZONTAL_DISPLAY_END as usize]) + 1;
        let last_line = u32::from(c[crtc::VERTICAL_DISPLAY_END as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 2) << 7)
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x40) << 3);
        let line_compare = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3);
        let maximum_scan_line = c[crtc::MAXIMUM_SCAN_LINE as usize];
        let row_height = u32::from(maximum_scan_line & 31) + 1;
        let mut repeat = 1 << (maximum_scan_line >> 7);
        // Graphics repeats a memory row while the row-scan counter advances.
        // Collapse it only when row-scan bits cannot change the memory address
        // and the preset does not begin partway through that row.
        if indexed256
            && c[crtc::MODE_CONTROL as usize] & 3 == 3
            && u32::from(c[crtc::PRESET_ROW_SCAN as usize] & 31) % row_height == 0
        {
            repeat *= row_height;
        }
        let split_repeat = line_compare < last_line && (line_compare + 1) % repeat != 0;
        let partial_repeat = (last_line + 1) % repeat != 0;
        let scanline_step = if split_repeat || partial_repeat {
            1
        } else {
            repeat
        };
        DisplayGeometry {
            size: [
                (columns * glyph_width).div_ceil(if indexed256 { 2 } else { 1 }),
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
