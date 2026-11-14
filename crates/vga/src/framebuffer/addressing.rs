//! CRTC address generation for native drawing, before palette interpretation.
//! The renderer uses the same counter, row-scan and serializer rules in WGSL.
use super::{location::Location, FramebufferGeometry};
use crate::{
    registers::{attribute, crtc, graphics, sequencer},
    VgaDisplayState,
};

pub(super) struct Addressing {
    state: VgaDisplayState,
    text: bool,
    indexed: bool,
}

impl Addressing {
    pub fn new(state: VgaDisplayState) -> Option<Self> {
        let mode = state.attributes[attribute::MODE_CONTROL as usize];
        let shift = state.graphics[graphics::MODE as usize];
        let text = mode & 1 == 0;
        let indexed = mode & 0x40 != 0;
        if (!text && indexed != (shift & 0x40 != 0))
            || (text && state.graphics[graphics::MISCELLANEOUS as usize] & 1 != 0)
        {
            return None;
        }
        Some(Self {
            state,
            text,
            indexed,
        })
    }

    pub fn indexed(&self) -> bool {
        self.indexed
    }

    pub fn geometry(&self) -> FramebufferGeometry {
        let size = self.state.geometry().output.size;
        if self.text {
            let rows = self.state.layout.vertical_display
                * self.state.crtc.scanlines_per_vertical_count() as u32
                / self.repeat()
                / self.row_height();
            FramebufferGeometry::Text {
                columns: self.state.crtc.horizontal_display() as u16,
                rows: rows as u16,
            }
        } else {
            FramebufferGeometry::Graphics {
                width: size[0] as u16,
                height: size[1] as u16,
            }
        }
    }

    fn crtc(&self, index: u8) -> u32 {
        u32::from(self.state.crtc.values[index as usize])
    }
    fn row_height(&self) -> u32 {
        (self.crtc(crtc::MAXIMUM_SCAN_LINE) & 31) + 1
    }
    fn repeat(&self) -> u32 {
        1 << (self.crtc(crtc::MAXIMUM_SCAN_LINE) >> 7)
    }
    fn count_divisor(&self) -> u32 {
        if self.crtc(crtc::UNDERLINE_LOCATION) & 0x20 != 0 {
            4
        } else if self.crtc(crtc::MODE_CONTROL) & 8 != 0 {
            2
        } else {
            1
        }
    }

    /// Counter, row scan, byte pan and pel pan at a physical scanline.
    fn scanline(&self, physical: u32) -> [u32; 4] {
        let preset = self.crtc(crtc::PRESET_ROW_SCAN);
        let split = (self.state.layout.line_compare + 1)
            * self.state.crtc.scanlines_per_vertical_count() as u32;
        let mut row = physical / self.repeat() + (preset & 31);
        let mut start = self.state.display_start;
        let mut byte_pan = (preset >> 5) & 3;
        let mut pel_pan =
            u32::from(self.state.attributes[attribute::HORIZONTAL_PANNING as usize] & 7);
        if self.indexed {
            pel_pan /= 2;
        }
        if physical >= split {
            row = (physical - split) / self.repeat();
            start = 0;
            if self.state.attributes[attribute::MODE_CONTROL as usize] & 0x20 != 0 {
                byte_pan = 0;
                pel_pan = 0;
            }
        }
        [
            start + row / self.row_height() * self.crtc(crtc::OFFSET) * 2,
            row % self.row_height(),
            byte_pan,
            pel_pan,
        ]
    }

    fn address(&self, counter: u32, row_scan: u32) -> u32 {
        let mask = self.state.layout.plane_address_mask();
        let counter = counter & mask;
        let mode = self.crtc(crtc::MODE_CONTROL);
        let mut address = if self.state.layout.linear_addressing {
            counter
        } else if self.crtc(crtc::UNDERLINE_LOCATION) & 0x40 != 0 {
            (counter << 2) | ((counter >> 12) & 3)
        } else if mode & 0x40 == 0 {
            (counter << 1) | ((counter >> if mode & 0x20 != 0 { 15 } else { 13 }) & 1)
        } else {
            counter
        };
        if mode & 1 == 0 {
            address = (address & !0x2000) | ((row_scan & 1) << 13);
        }
        if mode & 2 == 0 {
            address = (address & !0x4000) | ((row_scan & 2) << 13);
        }
        (address & mask) * 4
    }

    pub fn pixel(&self, x: u16, y: u16) -> Option<Location> {
        let FramebufferGeometry::Graphics { width, height } = self.geometry() else {
            return None;
        };
        if x >= width || y >= height {
            return None;
        }
        let [counter, row_scan, byte_pan, pel_pan] =
            self.scanline(u32::from(y) * self.state.geometry().scanline_step);
        let pixel = u32::from(x) + pel_pan;
        let clocking = self.state.sequencer[sequencer::CLOCKING_MODE as usize];
        let interval = if clocking & 0x10 != 0 {
            4
        } else if clocking & 4 != 0 {
            2
        } else {
            1
        };
        let clock = pixel / if self.indexed { 4 } else { 8 } + byte_pan;
        let rotation = (clock % interval) as u8;
        let address = self.address(
            counter + (clock - u32::from(rotation)) / self.count_divisor(),
            row_scan,
        );
        if self.indexed {
            Some(Location::Indexed {
                address: address + (pixel & 3),
                rotation,
            })
        } else {
            let interleaved = self.state.graphics[graphics::MODE as usize] & 0x20 != 0;
            Some(Location::Planar {
                address: address + if interleaved { (pixel & 7) / 4 } else { 0 },
                shift: if interleaved {
                    (6 - (pixel & 3) * 2) as u8
                } else {
                    (7 - (pixel & 7)) as u8
                },
                interleaved,
                rotation,
            })
        }
    }

    pub fn text_cell(&self, row: u16, column: u16) -> Option<u32> {
        let FramebufferGeometry::Text { columns, rows } = self.geometry() else {
            return None;
        };
        if row >= rows || column >= columns {
            return None;
        }
        let [counter, row_scan, byte_pan, _] =
            self.scanline(u32::from(row) * self.row_height() * self.repeat());
        Some(self.address(
            counter + byte_pan + u32::from(column) / self.count_divisor(),
            row_scan,
        ))
    }
}
