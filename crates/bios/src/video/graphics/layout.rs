//! BIOS layouts describe CPU-visible storage; scanout follows VGA registers.
use super::super::{modes, MODE};
use relocade_machine::MemoryView;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    Planar,
    Indexed256,
    Packed1,
    Packed2,
}

impl Layout {
    pub fn pixels_per_byte(self) -> u16 {
        match self {
            Self::Planar | Self::Packed1 => 8,
            Self::Packed2 => 4,
            Self::Indexed256 => 1,
        }
    }

    pub fn pixel_mask(self) -> u8 {
        match self {
            Self::Planar | Self::Packed1 => 1,
            Self::Packed2 => 3,
            Self::Indexed256 => 255,
        }
    }

    pub fn shift(self, x: u16) -> u8 {
        let pixels = self.pixels_per_byte();
        ((pixels - 1 - x % pixels) * (8 / pixels)) as u8
    }

    pub fn write_planes(self) -> u8 {
        match self {
            Self::Packed1 => 1,
            Self::Packed2 => 3,
            Self::Planar | Self::Indexed256 => 15,
        }
    }

    pub fn aperture(self) -> Range<u32> {
        match self {
            Self::Packed1 | Self::Packed2 => 0xb8000..0xc0000,
            Self::Planar | Self::Indexed256 => 0xa0000..0xb0000,
        }
    }

    pub fn row_offset(self, stride: u16, y: u16) -> u32 {
        match self {
            Self::Packed1 | Self::Packed2 => {
                u32::from(y & 1) * 0x2000 + u32::from(y / 2) * u32::from(stride)
            }
            Self::Planar | Self::Indexed256 => u32::from(y) * u32::from(stride),
        }
    }

    pub fn start_divisor(self) -> u16 {
        match self {
            Self::Planar | Self::Packed1 => 1,
            Self::Packed2 => 2,
            Self::Indexed256 => 4,
        }
    }
}

pub(super) struct Mode {
    pub width: u16,
    pub height: u16,
    pub page_bytes: u16,
    pub pages: u8,
    pub layout: Layout,
}

impl Mode {
    pub fn read(memory: &MemoryView<'_>) -> Option<Self> {
        let (width, height, page_bytes, pages, layout) =
            match memory.read_u8(MODE).expect("video mode") {
                modes::GRAPHICS_320X200_4 | modes::GRAPHICS_320X200_4_ALT => {
                    (320, 200, 0x4000, 1, Layout::Packed2)
                }
                modes::GRAPHICS_640X200_2 => (640, 200, 0x4000, 1, Layout::Packed1),
                modes::GRAPHICS_320X200_16 => (320, 200, 0x2000, 8, Layout::Planar),
                modes::GRAPHICS_640X200_16 => (640, 200, 0x4000, 4, Layout::Planar),
                modes::GRAPHICS_640X350_MONO | modes::GRAPHICS_640X350_16 => {
                    (640, 350, 0x8000, 2, Layout::Planar)
                }
                modes::GRAPHICS_640X480_2 | modes::GRAPHICS_640X480_16 => {
                    (640, 480, 0xa000, 1, Layout::Planar)
                }
                modes::GRAPHICS_320X200_256 => (320, 200, 64000, 1, Layout::Indexed256),
                _ => return None,
            };
        Some(Self {
            width,
            height,
            page_bytes,
            pages,
            layout,
        })
    }
}
