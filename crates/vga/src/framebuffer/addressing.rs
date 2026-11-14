//! Apply prepared display addressing to native drawing coordinates.
//! The renderer applies the same counter and serializer rules in WGSL.
use super::{location::Location, FramebufferGeometry};
use crate::{
    scanout::{VgaMode, VgaParameters},
    VgaDisplayState,
};

pub(super) struct Addressing<'a> {
    parameters: &'a VgaParameters,
    display_start: u32,
    geometry: FramebufferGeometry,
}

impl<'a> Addressing<'a> {
    pub fn new(state: &'a VgaDisplayState) -> Option<Self> {
        Some(Self {
            parameters: &state.parameters,
            display_start: state.display_start,
            geometry: state.parameters.framebuffer?,
        })
    }

    pub fn geometry(&self) -> FramebufferGeometry {
        self.geometry
    }

    /// Counter, row scan, byte pan and pel pan at a physical scanline.
    fn scanline(&self, physical: u32) -> [u32; 4] {
        let p = self.parameters;
        let mut row = physical / p.scan_repeat + p.preset_row;
        let mut start = self.display_start;
        let mut byte_pan = p.byte_pan;
        let mut pel_pan = p.pel_pan;
        if physical >= p.split_scanline {
            row = (physical - p.split_scanline) / p.scan_repeat;
            start = 0;
            if p.reset_panning {
                byte_pan = 0;
                pel_pan = 0;
            }
        }
        [
            start + row / p.row_height * p.stride,
            row % p.row_height,
            byte_pan,
            pel_pan,
        ]
    }

    fn address(&self, counter: u32, row_scan: u32) -> u32 {
        let pins = &self.parameters.pins;
        let counter = counter & pins.plane_mask;
        let address = (counter << pins.shift) | ((counter >> pins.wrap_shift) & pins.wrap_mask);
        let address = (address & !pins.row_scan_mask) | ((row_scan << 13) & pins.row_scan_mask);
        (address & pins.plane_mask) * 4
    }

    pub fn pixel(&self, x: u16, y: u16) -> Option<Location> {
        let FramebufferGeometry::Graphics { width, height } = self.geometry else {
            return None;
        };
        if x >= width || y >= height {
            return None;
        }
        let p = self.parameters;
        let [counter, row_scan, byte_pan, pel_pan] =
            self.scanline(u32::from(y) * p.geometry.scanline_step);
        let pixel = u32::from(x) + pel_pan;
        let clock = pixel / p.pixels_per_clock + byte_pan;
        let rotation = (clock % p.load_interval) as u8;
        let address = self.address(
            counter + (clock - u32::from(rotation)) / p.count_divisor,
            row_scan,
        );
        if p.mode == VgaMode::Indexed256 {
            Some(Location::Indexed {
                address: address + (pixel & 3),
                rotation,
            })
        } else {
            let interleaved = p.mode == VgaMode::Interleaved;
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
        let FramebufferGeometry::Text { columns, rows } = self.geometry else {
            return None;
        };
        if row >= rows || column >= columns {
            return None;
        }
        let [counter, row_scan, byte_pan, _] =
            self.scanline(u32::from(row) * self.parameters.text_scanlines);
        Some(self.address(
            counter + byte_pan + u32::from(column) / self.parameters.count_divisor,
            row_scan,
        ))
    }
}
