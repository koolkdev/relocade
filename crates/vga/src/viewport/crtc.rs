//! Translate prepared VGA addressing to a firmware image and program CRTC viewports.
use super::{Addressing, Viewport};
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    scanout::{DisplayConfiguration, PreparedScanout, VgaMode},
    Registers,
};

impl DisplayConfiguration {
    /// Use the same effective counters, address pins and panning as scanout.
    /// The card supplies its full programmed start and CPU addressing limit:
    /// a VGA aperture on DISPI, or banked physical memory on ET4000.
    pub fn crtc_viewport(
        &self,
        registers: &Registers,
        display_start: u32,
        accessible_bytes: u32,
    ) -> Option<Viewport> {
        let PreparedScanout::Vga { parameters: p, .. } = &self.scanout else {
            return None;
        };
        let memory_mode = registers.sequencer.values[sequencer::MEMORY_MODE as usize];
        let shift_mode = registers.graphics.values[graphics::MODE as usize];
        let map = registers.graphics.values[graphics::MISCELLANEOUS as usize];
        let physical_height = p.geometry.output.size[1] * p.geometry.scanline_step;
        if p.pins.row_scan_mask != 0
            || p.count_divisor != 1
            || p.load_interval != 1
            || memory_mode & 2 == 0
            || p.split_scanline < physical_height
            || p.stride == 0
        {
            return None;
        }
        let addressing = match p.mode {
            VgaMode::Text
                if map & 0x0c >= 8
                    && map & 3 == 2
                    && memory_mode & 0x0c == 0
                    && shift_mode & 0x70 == 0x10
                    && p.pins.shift == 1
                    && p.pins.wrap_shift == 15 =>
            {
                Addressing::Text([p.glyph_width, p.row_height])
            }
            VgaMode::Planar if map & 0x0f == 5 && memory_mode & 0x0c == 4 && p.pins.shift == 0 => {
                Addressing::Planar
            }
            VgaMode::Indexed256
                if map & 0x0f == 5
                    && memory_mode & 0x0c == 0x0c
                    && matches!(p.pins.shift, 0 | 2) =>
            {
                Addressing::Indexed256
            }
            _ => return None,
        };
        if (p.mode != VgaMode::Text && (p.glyph_width != 8 || p.preset_row != 0))
            || p.preset_row >= p.row_height
        {
            return None;
        }
        let row_scanlines = if p.mode == VgaMode::Text {
            p.scan_repeat
        } else {
            p.text_scanlines
        };
        if !physical_height.is_multiple_of(row_scanlines)
            || (p.mode != VgaMode::Text && p.geometry.scanline_step != row_scanlines)
        {
            return None;
        }
        // Bounded firmware images stop before shifted counter bits wrap in
        // physical memory. Banked cards are not restricted to one CPU window.
        let address_count = (p.pins.plane_mask + 1) >> p.pins.shift;
        // The 32-KiB text window leaves holes between ET4000 banks; those
        // windows do not form a contiguous logical text image.
        let accessible_bytes = if p.mode == VgaMode::Text {
            accessible_bytes.min(registers.graphics.memory_window().len() as u32)
        } else {
            accessible_bytes
        };
        let address_pixels = addressing.address_pixels();
        let address_bytes = addressing.address_bytes();
        let start = display_start + p.byte_pan;
        Some(Viewport {
            size: [p.geometry.output.size[0], physical_height / row_scanlines],
            addressing,
            pitch_granularity: 2 * address_bytes,
            pitch_limit: 255 * 2 * address_bytes,
            memory_bytes: (address_count * address_bytes).min(accessible_bytes),
            pitch: p.stride * address_bytes,
            origin: [
                (start % p.stride) * address_pixels[0] + p.pel_pan,
                (start / p.stride) * address_pixels[1] + p.preset_row,
            ],
        })
    }
}

impl Viewport {
    /// Program standard CRTC fields and return the full start counter. The card
    /// writes any high bits in the same effects operation, preserving its cursor.
    pub(crate) fn write_crtc(self, registers: &mut Registers) -> u32 {
        // Packed ET4000 CRTC addresses select four physical bytes. VGA text,
        // planar and indexed serializers use their decoded address units.
        let (address_pixels, address_bytes) = match self.addressing {
            Addressing::Packed(format) => ([4 / format.bytes_per_pixel(), 1], 4),
            addressing => (addressing.address_pixels(), addressing.address_bytes()),
        };
        let [x, y] = self.origin;
        let start = y / address_pixels[1] * (self.pitch / address_bytes) + x / address_pixels[0];
        let pan = (x % address_pixels[0]) as u8;
        let pan = match self.addressing {
            Addressing::Text([9, _]) => {
                if pan == 0 {
                    8
                } else {
                    pan - 1
                }
            }
            Addressing::Indexed256 => pan * 2,
            _ => pan,
        };
        let c = &mut registers.crtc.registers.values;
        let row_scan = match self.addressing {
            Addressing::Packed(_) => c[crtc::PRESET_ROW_SCAN as usize] & !0x60,
            _ => (y % address_pixels[1]) as u8,
        };
        c[crtc::OFFSET as usize] = (self.pitch / (2 * address_bytes)) as u8;
        c[crtc::START_ADDRESS_HIGH as usize] = (start >> 8) as u8;
        c[crtc::START_ADDRESS_LOW as usize] = start as u8;
        c[crtc::PRESET_ROW_SCAN as usize] = row_scan;
        registers.attribute.values[attribute::HORIZONTAL_PANNING as usize] = pan;
        start
    }
}
