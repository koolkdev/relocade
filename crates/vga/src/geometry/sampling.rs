//! Image sample spacing, including when repeated physical scanlines can share a row.
use crate::registers::{
    attribute,
    crtc::{self, Crtc},
    graphics, Registers,
};
use std::ops::Range;

pub(super) struct Sampling {
    pub dots_per_pixel: u32,
    pub scanlines_per_row: u32,
}

impl Sampling {
    /// `visible` is the unwrapped nonblank interval in physical scanlines.
    pub fn new(registers: &Registers, visible: &Range<u32>) -> Self {
        let attribute_mode = registers.attribute.values[attribute::MODE_CONTROL as usize];
        let graphics_mode = registers.graphics.values[graphics::MODE as usize];
        let indexed256 = attribute_mode & 0x41 == 0x41 && graphics_mode & 0x40 != 0;
        let graphics16 = attribute_mode & 0x41 == 1 && graphics_mode & 0x40 == 0;
        let crtc = &registers.crtc;
        let maximum_scan_line = crtc.values[crtc::MAXIMUM_SCAN_LINE as usize];
        let row_height = u32::from(maximum_scan_line & 31) + 1;
        let mut repeat = 1 << (maximum_scan_line >> 7);

        // Graphics repeats a memory row while the row-scan counter advances.
        // Row-scan address substitution or a partial preset prevents collapsing it.
        if (indexed256 || graphics16)
            && crtc.values[crtc::MODE_CONTROL as usize] & 3 == 3
            && u32::from(crtc.values[crtc::PRESET_ROW_SCAN as usize] & 31) % row_height == 0
        {
            repeat *= row_height;
        }
        Self {
            dots_per_pixel: if indexed256 { 2 } else { 1 },
            scanlines_per_row: scanline_step(crtc, visible, repeat),
        }
    }
}

fn scanline_step(crtc: &Crtc, visible: &Range<u32>, repeat: u32) -> u32 {
    let scanlines_per_count = crtc.scanlines_per_vertical_count() as u32;
    let active_height = crtc.vertical_display() as u32 * scanlines_per_count;
    let split = (crtc.line_compare() as u32 + 1) * scanlines_per_count;
    let total = crtc.vertical_total() as u32 * scanlines_per_count;
    let aligned = |line: u32| line.is_multiple_of(repeat);

    // Picture and image edges must fall between whole repeated groups.
    if !aligned(active_height) || !aligned(visible.start) || !aligned(visible.end) {
        return 1;
    }
    if split < active_height && !aligned(split) {
        return 1;
    }
    // Wrapping through an odd total can put paired scanlines on opposite sides
    // of counter zero even when the crop's own edges are aligned.
    if visible.end > total && !aligned(total) {
        return 1;
    }
    for blank in crtc.vertical_blanking() {
        if blank.is_empty() {
            continue;
        }
        for count in [blank.start, blank.end] {
            let line = count * scanlines_per_count;
            if line < active_height && !aligned(line) {
                return 1;
            }
        }
    }
    repeat
}
