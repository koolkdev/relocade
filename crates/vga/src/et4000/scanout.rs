//! Register-derived image addressing, independent of the CPU segment pointers.
use super::{
    registers::{CONFIGURATION, EXTENDED_START, OVERFLOW},
    Et4000Ax,
};
use crate::{
    registers::{attribute, crtc, graphics},
    scanout::{DisplayConfiguration, PreparedScanout},
    snapshot::Blanking,
    PackedLayout, VgaDisplayLayout,
};
use std::ops::Range;

pub(super) fn configuration(card: &Et4000Ax) -> DisplayConfiguration {
    let registers = &card.vga.registers;
    let blanked = !registers.attribute.display_enabled()
        || registers.sequencer.reset_active()
        || registers.sequencer.values[1] & 0x20 != 0;
    match card.prepare_packed_layout() {
        Some(layout) => DisplayConfiguration {
            scanout: PreparedScanout::PackedWithCrtcStart {
                layout,
                bytes_per_address: 4,
            },
            blanked,
            blanking: card.packed_blanking(),
        },
        None => DisplayConfiguration {
            scanout: PreparedScanout::vga(registers, card.prepare_vga_layout()),
            blanked,
            blanking: Default::default(),
        },
    }
}

impl Et4000Ax {
    fn prepare_vga_layout(&self) -> VgaDisplayLayout {
        let registers = &self.vga.registers;
        let mut layout = VgaDisplayLayout::vga(registers);
        let attribute_mode = registers.attribute.values[attribute::MODE_CONTROL as usize];
        layout.memory_bytes = self.memory.capacity();
        layout.cursor_address |=
            u32::from(self.extensions.crtc(EXTENDED_START).unwrap() & 0x0c) << 14;
        layout.vertical_display = self.vertical_display();
        layout.line_compare |= u32::from(self.extensions.crtc(OVERFLOW).unwrap() & 0x10) << 6;
        layout.vertical_blanking = self
            .vertical_blanking()
            .map(|range| [range.start, range.end]);
        // The packed serializer and Tseng's contiguous mapping use the
        // counter directly. IBM mapping retains byte/word pin rotation.
        layout.linear_addressing = self.extensions.crtc(CONFIGURATION).unwrap() & 0x20 != 0
            || (attribute_mode & 0x41 == 0x41
                && registers.graphics.values[graphics::MODE as usize] & 0x40 != 0);
        layout
    }

    pub(super) fn programmed_start(&self) -> u32 {
        u32::from(self.vga.registers.crtc.programmed_start())
            | (u32::from(self.extensions.crtc(EXTENDED_START).unwrap() & 3) << 16)
    }

    pub(super) fn display_start(&self) -> u32 {
        u32::from(self.vga.registers.crtc.display_start)
            | (u32::from(self.display_start_high) << 16)
    }

    fn prepare_packed_layout(&self) -> Option<PackedLayout> {
        let registers = &self.vga.registers;
        let attribute = &registers.attribute.values;
        if registers.graphics.values[graphics::MODE as usize] & 0x40 == 0
            || attribute[attribute::MODE_CONTROL as usize] & 0x41 != 1
        {
            return None;
        }
        let crtc = &registers.crtc.registers;
        let values = &crtc.values;
        // The DAC consumes one clock per pixel with dual-edge word output, or
        // two clocks per pixel with consecutive byte output (SC11483 modes 1/2).
        let width = crtc.horizontal_display() as u32
            * registers.sequencer.dots_per_character() as u32
            / self.ramdac.clocks_per_pixel();
        let repeat = (u32::from(values[crtc::MAXIMUM_SCAN_LINE as usize] & 31) + 1)
            << (values[crtc::MAXIMUM_SCAN_LINE as usize] >> 7);
        let height = self.vertical_display() * crtc.scanlines_per_vertical_count() as u32 / repeat;
        // The linear counter addresses four adjacent physical bytes. Row
        // Offset advances it by twice the programmed value (data book p.123).
        let pitch = u32::from(values[crtc::OFFSET as usize]) * 8;
        let format = self.ramdac.format();
        let pan = u32::from(attribute[attribute::HORIZONTAL_PANNING as usize] & 7)
            * format.bytes_per_pixel();
        let byte_pan = u32::from((values[crtc::PRESET_ROW_SCAN as usize] >> 5) & 3) * 4;
        PackedLayout::wrapping(
            [width, height],
            format,
            pitch,
            byte_pan + pan,
            self.memory.capacity(),
        )
    }

    fn packed_blanking(&self) -> Blanking {
        let registers = &self.vga.registers;
        let crtc = &registers.crtc.registers;
        let skew = crtc.horizontal_display_skew() as u32;
        let dots = registers.sequencer.dots_per_character() as u32;
        let clocks = self.ramdac.clocks_per_pixel();
        let maximum_row = crtc.values[crtc::MAXIMUM_SCAN_LINE as usize];
        let repeat = (u32::from(maximum_row & 31) + 1) << (maximum_row >> 7);
        let scanlines = crtc.scanlines_per_vertical_count() as u32;
        Blanking {
            horizontal: crtc.horizontal_blanking().map(|range| {
                (range.start.saturating_sub(skew) * dots).div_ceil(clocks)
                    ..(range.end.saturating_sub(skew) * dots).div_ceil(clocks)
            }),
            vertical: self.vertical_blanking().map(|range| {
                (range.start * scanlines).div_ceil(repeat)..(range.end * scanlines).div_ceil(repeat)
            }),
        }
    }

    fn vertical_display(&self) -> u32 {
        self.vga.registers.crtc.registers.vertical_display() as u32
            + (u32::from(self.extensions.crtc(OVERFLOW).unwrap() & 4) << 8)
    }

    fn vertical_blanking(&self) -> [Range<u32>; 2] {
        let crtc = &self.vga.registers.crtc.registers;
        let overflow = self.extensions.crtc(OVERFLOW).unwrap();
        // ET4000 extends the start and total to eleven bits and compares all
        // eight end bits (data book pp.124–125, 135).
        crtc::blanking_intervals(
            crtc.vertical_blank_start() | (u32::from(overflow & 1) << 10),
            u32::from(crtc.values[crtc::VERTICAL_BLANK_END as usize]),
            255,
            crtc.vertical_total() as u32 + (u32::from(overflow & 2) << 9),
        )
    }
}
