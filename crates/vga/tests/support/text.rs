//! CPU reference for rendering and inspection tests. Decodes raw captures
//! independently of the production shader and its transfer representation.
#![allow(dead_code)]
use relocade_vga::{
    registers::{attribute, crtc, graphics, sequencer},
    DisplaySnapshot,
};

/// Effective text settings derived solely from one captured display state.
#[derive(Clone, Debug)]
pub struct Text {
    pub layout: Layout,
    pub font_a: u16,
    pub font_b: u16,
    pub dual_font: bool,
    pub cursor_address: u16,
    pub cursor_start: u8,
    pub cursor_end: u8,
    pub attribute_mode: u8,
    /// Packed little-endian RGBA8 values after attribute selection and DAC masking.
    pub colors: [u32; 16],
    pub blink_on: bool,
    pub blanked: bool,
}

/// Text fetch geometry and addressing. Colors, cursor and blink remain in
/// `Text`; memory remains in the snapshot's physical planes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: [u32; 2],
    pub columns: u32,
    pub glyph_width: u32,
    pub glyph_height: u32,
    pub start_address: u16,
    pub stride: u16,
    pub preset_row: u32,
    pub byte_panning: u32,
    pub pel_panning: u32,
    pub line_compare: u32,
    pub scan_repeat: u32,
    pub output_step: u32,
    pub address_mode: u8,
    /// CRTC underline register; its high bits also control memory addressing.
    pub underline: u8,
    pub count_divisor: u32,
    pub reset_panning: bool,
}

/// Address-counter and character-row inputs for one output scanline.
pub struct Scanline {
    pub counter: u16,
    pub glyph_row: u8,
    pub pan: u32,
}

impl Text {
    pub fn new(snapshot: &DisplaySnapshot) -> Self {
        let r = &snapshot.registers;
        let c = &r.crtc.values;
        let a = &r.attribute.values;
        let s = &r.sequencer.values;
        let glyph_width = if s[sequencer::CLOCKING_MODE as usize] & 1 != 0 {
            8
        } else {
            9
        };
        let columns = u32::from(c[crtc::HORIZONTAL_DISPLAY_END as usize]) + 1;
        let lines = u32::from(c[crtc::VERTICAL_DISPLAY_END as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 2) << 7)
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x40) << 3);
        let vertical_divisor = 1 << ((c[crtc::MODE_CONTROL as usize] >> 2) & 1);
        let lines = (lines + 1) * vertical_divisor - 1;
        let scan_repeat = 1 << (c[crtc::MAXIMUM_SCAN_LINE as usize] >> 7);
        let maps = s[sequencer::CHARACTER_MAP_SELECT as usize];
        let extended = s[sequencer::MEMORY_MODE as usize] & 2 != 0;
        let font_mask = if extended { 0xffff } else { 0x3fff };
        let font_a = (((u16::from(maps) >> 2) & 3) * 0x4000
            + ((u16::from(maps) >> 5) & 1) * 0x2000)
            & font_mask;
        let font_b =
            ((u16::from(maps) & 3) * 0x4000 + ((u16::from(maps) >> 4) & 1) * 0x2000) & font_mask;
        let mode = a[attribute::MODE_CONTROL as usize];
        let select = a[attribute::COLOR_SELECT as usize];
        let colors = std::array::from_fn(|color| {
            let color = color & usize::from(a[attribute::COLOR_PLANE_ENABLE as usize]);
            let mut index = a[color];
            if mode & 0x80 != 0 {
                index = (index & 15) | ((select & 3) << 4);
            }
            index = (index | ((select & 12) << 4)) & r.dac.mask;
            let rgb =
                r.dac.colors[usize::from(index)].map(|v| ((u32::from(v) * 255 + 31) / 63) as u8);
            u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255])
        });
        let line_compare = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3);
        let line_compare = (line_compare + 1) * vertical_divisor - 1;
        // Keep physical rows when a split interrupts a pair or the display ends
        // with half a pair. Only complete pairs have equal vertical weights.
        let output_step = if scan_repeat == 2
            && (lines.is_multiple_of(2) || (line_compare < lines && line_compare.is_multiple_of(2)))
        {
            1
        } else {
            scan_repeat
        };
        let pan = u32::from(a[attribute::HORIZONTAL_PANNING as usize]);
        Text {
            layout: Layout {
                size: [columns * glyph_width, (lines + 1).div_ceil(output_step)],
                columns,
                glyph_width,
                glyph_height: u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 31) + 1,
                start_address: u16::from_be_bytes([c[12], c[13]]),
                stride: u16::from(c[crtc::OFFSET as usize]) * 2,
                preset_row: u32::from(c[crtc::PRESET_ROW_SCAN as usize] & 31),
                byte_panning: u32::from((c[crtc::PRESET_ROW_SCAN as usize] >> 5) & 3),
                pel_panning: if glyph_width == 9 {
                    if pan < 8 {
                        pan + 1
                    } else {
                        0
                    }
                } else {
                    pan & 7
                },
                line_compare,
                scan_repeat,
                output_step,
                address_mode: c[crtc::MODE_CONTROL as usize],
                underline: c[crtc::UNDERLINE_LOCATION as usize],
                count_divisor: if c[crtc::UNDERLINE_LOCATION as usize] & 0x20 != 0 {
                    4
                } else if c[crtc::MODE_CONTROL as usize] & 8 != 0 {
                    2
                } else {
                    1
                },
                reset_panning: mode & 0x20 != 0,
            },
            font_a,
            font_b,
            dual_font: extended && font_a != font_b,
            cursor_address: u16::from_be_bytes([c[14], c[15]]),
            cursor_start: c[crtc::CURSOR_START as usize],
            cursor_end: c[crtc::CURSOR_END as usize],
            attribute_mode: mode,
            colors,
            blink_on: snapshot.blink_on,
            blanked: s[sequencer::CLOCKING_MODE as usize] & 0x20 != 0
                || r.attribute.address() & attribute::PALETTE_ADDRESS_SOURCE == 0
                || mode & 1 != 0
                || r.graphics.values[graphics::MISCELLANEOUS as usize] & 1 != 0,
        }
    }
}

impl Layout {
    /// Nominal 4:3 VGA display proportions; duplicated scanlines are represented
    /// by pixel aspect ratio. Physical monitor/raster reconstruction is separate.
    pub fn pixel_aspect_ratio(&self) -> (u32, u32) {
        let (width, height) = (self.size[1] * 4, self.size[0] * 3);
        let (mut a, mut b) = (width, height);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        (width / a, height / a)
    }

    pub fn scanline(&self, y: u32) -> Scanline {
        let physical_row = y * self.output_step;
        let split = physical_row > self.line_compare;
        let row = if split {
            (physical_row - self.line_compare - 1) / self.scan_repeat
        } else {
            physical_row / self.scan_repeat + self.preset_row
        };
        let start = if split {
            0
        } else {
            u32::from(self.start_address)
        };
        Scanline {
            counter: (start
                + (row / self.glyph_height) * u32::from(self.stride)
                + self.byte_panning) as u16,
            glyph_row: (row % self.glyph_height) as u8,
            pan: if split && self.reset_panning {
                0
            } else {
                self.pel_panning
            },
        }
    }

    /// Translate CRTC counter outputs to physical plane addresses (IBM p2-75).
    pub fn address(&self, counter: u16, row_scan: u8) -> u16 {
        let mode = self.address_mode;
        let mut address = if self.underline & 0x40 != 0 {
            (counter << 2) | ((counter >> 12) & 3)
        } else if mode & 0x40 == 0 {
            (counter << 1) | ((counter >> if mode & 0x20 != 0 { 15 } else { 13 }) & 1)
        } else {
            counter
        };
        if mode & 1 == 0 {
            address = (address & !0x2000) | (u16::from(row_scan & 1) << 13);
        }
        if mode & 2 == 0 {
            address = (address & !0x4000) | (u16::from(row_scan & 2) << 13);
        }
        address
    }
}

impl Text {
    pub fn font_address(&self, character: u8, attribute: u8, row: u8) -> u16 {
        let bank = if self.dual_font && attribute & 8 != 0 {
            self.font_a
        } else {
            self.font_b
        };
        bank + u16::from(character) * 32 + u16::from(row)
    }
}
