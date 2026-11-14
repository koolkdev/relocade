//! Bounded logical images in VGA text, planar and chain-four modes.
//! Pitch is measured in CPU aperture bytes, not interleaved physical VRAM bytes.
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    Registers,
};

/// A firmware viewport in the standard VGA memory layouts. CGA row banking,
/// split screens and custom serializer/address modes have no such linear image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyLayout {
    size: [u32; 2],
    // Pixels and CPU bytes represented by one CRTC memory address.
    unit: [u32; 2],
    unit_bytes: u32,
    memory_bytes: u32,
    pitch: u32,
    origin: [u32; 2],
}

impl LegacyLayout {
    /// Read programmed CRTC addresses, including writes not yet latched by retrace.
    /// The caller must first exclude any active card-specific packed layout.
    pub fn from_registers(registers: &Registers) -> Option<Self> {
        let crtc = &registers.crtc.registers;
        let c = &crtc.values;
        let sequencer = &registers.sequencer.values;
        let graphics = &registers.graphics.values;
        let attributes = &registers.attribute.values;
        let clocking = sequencer[sequencer::CLOCKING_MODE as usize];
        let memory_mode = sequencer[sequencer::MEMORY_MODE as usize];
        let address_mode = c[crtc::MODE_CONTROL as usize];
        let underline = c[crtc::UNDERLINE_LOCATION as usize];
        let shift = graphics[graphics::MODE as usize];
        let map = graphics[graphics::MISCELLANEOUS as usize];
        let mode = attributes[attribute::MODE_CONTROL as usize] & 0x41;
        let preset = c[crtc::PRESET_ROW_SCAN as usize];
        let row_height = u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 31) + 1;
        let repeat = 1 << (c[crtc::MAXIMUM_SCAN_LINE as usize] >> 7);
        let dots = if clocking & 1 == 0 { 9 } else { 8 };
        let physical_height =
            crtc.vertical_display() as u32 * crtc.scanlines_per_vertical_count() as u32;
        let split = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(c[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3);
        if address_mode & 0x0b != 3
            || underline & 0x20 != 0
            || clocking & 0x14 != 0
            || memory_mode & 2 == 0
            || (split + 1) * (crtc.scanlines_per_vertical_count() as u32) < physical_height
        {
            return None;
        }
        let (unit, unit_bytes, memory_bytes, dots_per_pixel) = match mode {
            0 if map & 1 == 0
                && map & 0x0c >= 8
                && memory_mode & 0x0c == 0
                && shift & 0x70 == 0x10
                && address_mode & 0x60 == 0x20
                && underline & 0x40 == 0 =>
            {
                ([dots, row_height], 2, crate::TEXT_MEMORY_BYTES, 1)
            }
            1 if map & 0x0f == 5
                && memory_mode & 0x0c == 4
                && shift & 0x60 == 0
                && address_mode & 0x40 != 0
                && underline & 0x40 == 0 =>
            {
                ([8, 1], 1, 65536, 1)
            }
            0x41 if map & 0x0f == 5
                && memory_mode & 0x0c == 0x0c
                && shift & 0x60 == 0x40
                && underline & 0x40 != 0 =>
            {
                ([4, 1], 4, 65536, 2)
            }
            _ => return None,
        };
        if (mode != 0 && (dots != 8 || preset & 31 != 0)) || u32::from(preset & 31) >= row_height {
            return None;
        }
        let pitch_units = u32::from(c[crtc::OFFSET as usize]) * 2;
        if pitch_units == 0 {
            return None;
        }
        let pan = u32::from(attributes[attribute::HORIZONTAL_PANNING as usize]);
        let pan = if mode == 0x41 {
            (pan & 7) / 2
        } else if mode == 0 && dots == 9 {
            if pan < 8 {
                pan + 1
            } else {
                0
            }
        } else {
            pan & 7
        };
        let start = u32::from(registers.crtc.programmed_start()) + u32::from((preset >> 5) & 3);
        let scanline_step = repeat * if mode == 0 { 1 } else { row_height };
        if !physical_height.is_multiple_of(scanline_step) {
            return None;
        }
        Some(Self {
            size: [
                crtc.horizontal_display() as u32 * dots / dots_per_pixel,
                physical_height / scanline_step,
            ],
            unit,
            unit_bytes,
            memory_bytes,
            pitch: pitch_units * unit_bytes,
            origin: [
                (start % pitch_units) * unit[0] + pan,
                (start / pitch_units) * unit[1] + u32::from(preset & 31),
            ],
        })
    }

    pub fn pitch(self) -> u32 {
        self.pitch
    }

    pub fn virtual_width(self) -> u32 {
        self.pitch / self.unit_bytes * self.unit[0]
    }

    /// Text heights count glyph scanlines, excluding hardware double scanning.
    pub fn virtual_height(self) -> u32 {
        self.memory_bytes / self.pitch * self.unit[1]
    }

    pub fn origin(self) -> [u32; 2] {
        self.origin
    }

    pub fn pitch_for_width(self, pixels: u32) -> u32 {
        pixels.max(self.size[0]).div_ceil(self.unit[0]) * self.unit_bytes
    }

    pub fn max_pitch(self) -> u32 {
        let granularity = 2 * self.unit_bytes;
        let rows = self.size[1].div_ceil(self.unit[1]);
        (self.memory_bytes / rows / granularity).min(255) * granularity
    }

    /// Round to the CRTC Offset unit without changing the pixel origin.
    pub fn with_pitch(mut self, pitch: u32) -> Option<Self> {
        let granularity = 2 * self.unit_bytes;
        self.pitch = pitch
            .max(self.pitch_for_width(0))
            .div_ceil(granularity)
            .checked_mul(granularity)?;
        (self.pitch <= self.max_pitch())
            .then_some(self)?
            .with_origin(self.origin)
    }

    /// Maximum capability is independent of the current viewport origin.
    pub fn maximum(mut self) -> Option<Self> {
        self.origin = [0, 0];
        self.with_pitch(self.max_pitch())
    }

    /// Move within complete addressable rows. Coordinates are pixels, including
    /// glyph dots and glyph scanlines in text modes.
    pub fn with_origin(mut self, origin: [u32; 2]) -> Option<Self> {
        if origin[0].checked_add(self.size[0])? > self.virtual_width()
            || origin[1].checked_add(self.size[1])? > self.virtual_height()
        {
            return None;
        }
        self.origin = origin;
        Some(self)
    }

    /// Register programming for the checked viewport. Byte panning is folded
    /// into the start address; fine text and pixel panning remain independent.
    pub fn crtc_registers(self) -> [(u8, u8); 4] {
        let [x, y] = self.origin;
        let start = y / self.unit[1] * (self.pitch / self.unit_bytes) + x / self.unit[0];
        [
            (crtc::OFFSET, (self.pitch / (2 * self.unit_bytes)) as u8),
            (crtc::START_ADDRESS_HIGH, (start >> 8) as u8),
            (crtc::START_ADDRESS_LOW, start as u8),
            (crtc::PRESET_ROW_SCAN, (y % self.unit[1]) as u8),
        ]
    }

    pub fn horizontal_panning(self) -> u8 {
        let pan = (self.origin[0] % self.unit[0]) as u8;
        match self.unit[0] {
            9 => {
                if pan == 0 {
                    8
                } else {
                    pan - 1
                }
            }
            4 => pan * 2,
            _ => pan,
        }
    }
}
