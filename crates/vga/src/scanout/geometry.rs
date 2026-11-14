//! Output geometry and row collapsing, prepared from display programming.
use super::{VgaDisplayLayout, VgaGeometry};
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    DisplayGeometry, Registers,
};

impl VgaGeometry {
    /// Output geometry for text, four-bit and eight-bit graphics layouts.
    pub(super) fn prepare(registers: &Registers, layout: &VgaDisplayLayout) -> Self {
        let clocking_mode = registers.sequencer.values[sequencer::CLOCKING_MODE as usize];
        let dots_per_character = if clocking_mode & 1 != 0 { 8 } else { 9 };
        let attribute_mode = registers.attribute.values[attribute::MODE_CONTROL as usize];
        let graphics_mode = registers.graphics.values[graphics::MODE as usize];
        let indexed256 = attribute_mode & 0x41 == 0x41 && graphics_mode & 0x40 != 0;
        let graphics16 = attribute_mode & 0x41 == 1 && graphics_mode & 0x40 == 0;
        let dots_per_pixel = if indexed256 { 2 } else { 1 };
        let columns = registers.crtc.registers.horizontal_display() as u32;
        let physical_height = layout.vertical_display
            * registers.crtc.registers.scanlines_per_vertical_count() as u32;
        let scanline_step =
            Self::scanline_step(registers, layout, physical_height, indexed256 || graphics16);
        let size = [
            (columns * dots_per_character).div_ceil(dots_per_pixel),
            physical_height / scanline_step,
        ];
        VgaGeometry {
            output: DisplayGeometry {
                size,
                pixel_aspect_ratio: nominal_aspect(size),
            },
            scanline_step,
        }
    }

    /// Collapse repeated rows only when every visible boundary falls between them.
    fn scanline_step(
        registers: &Registers,
        layout: &VgaDisplayLayout,
        physical_height: u32,
        graphics: bool,
    ) -> u32 {
        let c = &registers.crtc.registers.values;
        let maximum_scan_line = c[crtc::MAXIMUM_SCAN_LINE as usize];
        let row_height = u32::from(maximum_scan_line & 31) + 1;
        let mut repeat = 1 << (maximum_scan_line >> 7);
        // Graphics repeats a memory row while the row-scan counter advances.
        // Collapse it only when row-scan bits cannot change the memory address
        // and the preset does not begin partway through that row.
        if graphics
            && c[crtc::MODE_CONTROL as usize] & 3 == 3
            && u32::from(c[crtc::PRESET_ROW_SCAN as usize] & 31) % row_height == 0
        {
            repeat *= row_height;
        }

        let scanlines_per_count = registers.crtc.registers.scanlines_per_vertical_count() as u32;
        let split_line = (layout.line_compare + 1) * scanlines_per_count;
        let blanking_edges = layout
            .vertical_blanking
            .into_iter()
            .filter(|[start, end]| start < end)
            .flatten()
            .map(|count| count * scanlines_per_count);

        // The image end, split and blanking edges must not cut a repeated row.
        // Use exclusive boundaries throughout, measured in physical scanlines.
        if [physical_height, split_line]
            .into_iter()
            .chain(blanking_edges)
            .filter(|&line| line <= physical_height)
            .all(|line| line.is_multiple_of(repeat))
        {
            repeat
        } else {
            1
        }
    }
}

// VGA targets nominal 4:3 proportions under the frame-snapshot approximation.
fn nominal_aspect(size: [u32; 2]) -> (u32, u32) {
    let (width, height) = (size[1] * 4, size[0] * 3);
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (width / a, height / a)
}
