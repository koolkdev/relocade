use crate::{memory::VideoMemory, registers::Registers};

/// Text serialization: planes zero/one select a glyph in plane two and its colors.
pub(super) struct Text<'a> {
    memory: &'a VideoMemory,
    registers: &'a Registers,
    blink_on: bool,
}

impl<'a> Text<'a> {
    pub fn new(memory: &'a VideoMemory, registers: &'a Registers, blink_on: bool) -> Self {
        Self {
            memory,
            registers,
            blink_on,
        }
    }

    pub fn pixel(&self, row_address: u16, column: u16, row_scan: u8, dot: u8) -> u8 {
        let crtc = &self.registers.crtc;
        let sequencer = &self.registers.sequencer;
        let attribute = &self.registers.attribute;
        let counter = row_address.wrapping_add(column);
        let cell = self.memory.cell(crtc.display_address(counter, row_scan));
        let foreground = cell.attribute & if sequencer.separate_fonts() { 7 } else { 15 };
        let background = (cell.attribute >> 4) & if attribute.blink_enabled() { 7 } else { 15 };
        let glyph_address = sequencer.font_base(cell.attribute)
            + u16::from(cell.character) * 32
            + u16::from(row_scan);
        let bits = self.memory.plane(2, glyph_address);
        let extend_ninth = attribute.line_graphics() && (0xc0..=0xdf).contains(&cell.character);
        let glyph_dot = if dot < 8 {
            bits & (0x80 >> dot) != 0
        } else {
            extend_ninth && bits & 1 != 0
        };
        let underline = cell.attribute & 0x77 == 1
            && row_scan == crtc.underline_scan()
            && (dot < 8 || extend_ninth);
        let visible = self.blink_on || !attribute.blink_enabled() || cell.attribute & 0x80 == 0;
        let cursor_skew = u16::from((crtc.cursor_end() >> 5) & 3);
        let cursor = self.blink_on
            && crtc.cursor_start() & 0x20 == 0
            && column >= cursor_skew
            && counter.wrapping_sub(cursor_skew) == crtc.cursor_address()
            && (crtc.cursor_start() & 31..=crtc.cursor_end() & 31).contains(&row_scan);
        if cursor || (visible && (glyph_dot || underline)) {
            foreground
        } else {
            background
        }
    }
}
