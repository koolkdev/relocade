//! VGA display inputs and geometry decoded from captured register values.
use crate::{
    registers::{
        attribute,
        crtc::{self, CrtcRegisters},
        graphics, sequencer, LegacyRegisters,
    },
    DisplayGeometry,
};

/// VGA display registers and latched state. CPU register indices, transfer phases
/// and the shared DAC are not part of this payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgaDisplayState {
    pub sequencer: [u8; 5],
    pub crtc: CrtcRegisters,
    pub graphics: [u8; 9],
    pub attributes: [u8; 0x15],
    pub miscellaneous: u8,
    pub display_enabled: bool,
    /// Start address latched at the last serviced VGA retrace.
    pub display_start: u16,
    /// Cursor phase toggles every eight retraces.
    pub cursor_blink_on: bool,
    /// Text and graphics attribute phase toggles every sixteen retraces.
    pub attribute_blink_on: bool,
}

impl VgaDisplayState {
    pub(crate) fn capture(registers: &LegacyRegisters, retrace_count: u64) -> Self {
        Self {
            sequencer: registers.sequencer.state.values,
            crtc: registers.crtc.state.registers,
            graphics: registers.graphics.state.values,
            attributes: registers.attribute.state.values,
            miscellaneous: registers.miscellaneous,
            display_enabled: registers.attribute.state.address() & 0x20 != 0,
            display_start: registers.crtc.state.display_start,
            // IBM VGA: cursor VSYNC/16 and attribute VSYNC/32, both 50% duty.
            cursor_blink_on: (retrace_count / 8).is_multiple_of(2),
            attribute_blink_on: (retrace_count / 16).is_multiple_of(2),
        }
    }
}

/// VGA row interpretation accompanying the common output geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgaGeometry {
    pub output: DisplayGeometry,
    /// Physical scanlines represented by each image row.
    pub scanline_step: u32,
}

impl VgaDisplayState {
    /// Output geometry for text, four-bit and eight-bit graphics layouts.
    pub fn geometry(&self) -> VgaGeometry {
        let clocking_mode = self.sequencer[sequencer::CLOCKING_MODE as usize];
        let dots_per_character = if clocking_mode & 1 != 0 { 8 } else { 9 };
        let attribute_mode = self.attributes[attribute::MODE_CONTROL as usize];
        let graphics_mode = self.graphics[graphics::MODE as usize];
        let indexed256 = attribute_mode & 0x41 == 0x41 && graphics_mode & 0x40 != 0;
        let graphics16 = attribute_mode & 0x41 == 1 && graphics_mode & 0x40 == 0;
        let dots_per_pixel = if indexed256 { 2 } else { 1 };
        let columns = self.crtc.horizontal_display() as u32;
        let physical_height =
            self.crtc.vertical_display() as u32 * self.crtc.scanlines_per_vertical_count() as u32;
        let scanline_step = self.scanline_step(physical_height, indexed256 || graphics16);
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
    fn scanline_step(&self, physical_height: u32, graphics: bool) -> u32 {
        let c = &self.crtc.values;
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

        let scanlines_per_count = self.crtc.scanlines_per_vertical_count() as u32;
        let line_compare = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3);
        let split_line = (line_compare + 1) * scanlines_per_count;
        let blanking_edges = self
            .crtc
            .vertical_blanking()
            .into_iter()
            .filter(|range| !range.is_empty())
            .flat_map(|range| [range.start, range.end])
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
