//! Register-derived image addressing, independent of the CPU segment pointers.
use super::{
    registers::{EXTENDED_START, OVERFLOW},
    Et4000Ax,
};
use crate::{
    registers::{attribute, crtc, graphics},
    scanout::{DisplayConfiguration, PreparedScanout},
    snapshot::Blanking,
    PackedLayout, PixelFormat,
};

pub(super) fn configuration(card: &Et4000Ax) -> DisplayConfiguration {
    let registers = &card.vga.registers;
    let blanked = !registers.attribute.display_enabled()
        || registers.sequencer.reset_active()
        || registers.sequencer.values[1] & 0x20 != 0;
    match card.packed_layout() {
        Some(layout) => DisplayConfiguration {
            scanout: PreparedScanout::PackedWithCrtcStart {
                layout,
                bytes_per_address: 4,
            },
            blanked,
            blanking: card.packed_blanking(),
        },
        None => DisplayConfiguration {
            scanout: PreparedScanout::Vga {
                // Keep VGA's serializer, row repeats, split screen and blanking.
                // ET4000 changes how its CRTC counter addresses physical bytes.
                linear_addressing: registers.attribute.values[attribute::MODE_CONTROL as usize]
                    & 0x41
                    == 0x41
                    && registers.graphics.values[graphics::MODE as usize] & 0x40 != 0,
            },
            blanked,
            blanking: Default::default(),
        },
    }
}

impl Et4000Ax {
    pub(super) fn programmed_start(&self) -> u32 {
        u32::from(self.vga.registers.crtc.programmed_start())
            | (u32::from(self.extensions.crtc(EXTENDED_START).unwrap() & 3) << 16)
    }

    pub(super) fn display_start(&self) -> u32 {
        u32::from(self.vga.registers.crtc.display_start)
            | (u32::from(self.display_start_high) << 16)
    }

    fn packed_layout(&self) -> Option<PackedLayout> {
        let registers = &self.vga.registers;
        let attribute = &registers.attribute.values;
        if registers.graphics.values[graphics::MODE as usize] & 0x40 == 0
            || attribute[attribute::MODE_CONTROL as usize] & 0x41 != 1
        {
            return None;
        }
        let crtc = &registers.crtc.registers;
        let values = &crtc.values;
        let width =
            crtc.horizontal_display() as u32 * registers.sequencer.dots_per_character() as u32;
        let repeat = (u32::from(values[crtc::MAXIMUM_SCAN_LINE as usize] & 31) + 1)
            << (values[crtc::MAXIMUM_SCAN_LINE as usize] >> 7);
        let vertical_display = crtc.vertical_display() as u32
            + (u32::from(self.extensions.crtc(OVERFLOW).unwrap() & 4) << 8);
        let height = vertical_display * crtc.scanlines_per_vertical_count() as u32 / repeat;
        // The linear counter addresses four adjacent physical bytes. Row
        // Offset advances it by twice the programmed value (data book p.123).
        let pitch = u32::from(values[crtc::OFFSET as usize]) * 8;
        let pan = u32::from(attribute[attribute::HORIZONTAL_PANNING as usize] & 7);
        let byte_pan = u32::from((values[crtc::PRESET_ROW_SCAN as usize] >> 5) & 3) * 4;
        PackedLayout::wrapping(
            [width, height],
            PixelFormat::Indexed8,
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
        let overflow = self.extensions.crtc(OVERFLOW).unwrap();
        // ET4000 extends the start and total to eleven bits and compares all
        // eight end bits (data book pp.124–125, 135).
        let vertical = crtc::blanking_intervals(
            crtc.vertical_blank_start() | (u32::from(overflow & 1) << 10),
            u32::from(crtc.values[crtc::VERTICAL_BLANK_END as usize]),
            255,
            crtc.vertical_total() as u32 + (u32::from(overflow & 2) << 9),
        );
        let maximum_row = crtc.values[crtc::MAXIMUM_SCAN_LINE as usize];
        let repeat = (u32::from(maximum_row & 31) + 1) << (maximum_row >> 7);
        let scanlines = crtc.scanlines_per_vertical_count() as u32;
        Blanking {
            horizontal: crtc.horizontal_blanking().map(|range| {
                range.start.saturating_sub(skew) * dots..range.end.saturating_sub(skew) * dots
            }),
            vertical: vertical.map(|range| {
                (range.start * scanlines).div_ceil(repeat)..(range.end * scanlines).div_ceil(repeat)
            }),
        }
    }
}
