//! BIOS graphics layouts and native scanline operations through the VGA aperture.
mod access;
mod pixel;
pub(super) use pixel::Pixel;

use super::{modes, MODE};
use crate::screen::Scroll;
use access::PlanarAccess;
use relocade_machine::{IoBus, MemoryView};

struct Mode {
    width: u16,
    height: u16,
    page_bytes: u16,
    pages: u8,
    planar: bool,
}

impl Mode {
    fn read(memory: &MemoryView<'_>) -> Option<Self> {
        let (width, height, page_bytes, pages, planar) =
            match memory.read_u8(MODE).expect("video mode") {
                modes::GRAPHICS_320X200_16 => (320, 200, 0x2000, 8, true),
                modes::GRAPHICS_640X480_16 => (640, 480, 0xa000, 1, true),
                modes::GRAPHICS_320X200_256 => (320, 200, 64000, 1, false),
                _ => return None,
            };
        Some(Self {
            width,
            height,
            page_bytes,
            pages,
            planar,
        })
    }

    fn page(self, page: u8) -> Option<Page> {
        (page < self.pages).then(|| Page {
            offset: u16::from(page) * self.page_bytes,
            mode: self,
        })
    }
}

pub(crate) struct Page {
    mode: Mode,
    offset: u16,
}

impl Page {
    pub fn locate(memory: &MemoryView<'_>, page: u8) -> Option<Self> {
        Mode::read(memory)?.page(page)
    }

    pub fn offset(&self) -> u16 {
        self.offset
    }

    pub fn display_start(&self) -> u16 {
        // Planar presets use byte addresses; mode 13h uses doubleword addresses.
        self.offset / if self.mode.planar { 1 } else { 4 }
    }

    pub fn columns(&self) -> u16 {
        self.mode.width / 8
    }

    pub fn rows(&self, font_height: u16) -> u16 {
        self.mode.height.checked_div(font_height).unwrap_or(0)
    }

    fn address(&self, x: u16, y: u16) -> u32 {
        let pixels_per_byte = if self.mode.planar { 8 } else { 1 };
        0xa0000
            + u32::from(self.offset)
            + u32::from(y) * u32::from(self.mode.width / pixels_per_byte)
            + u32::from(x / pixels_per_byte)
    }

    pub fn scroll(
        &self,
        memory: &mut MemoryView<'_>,
        io: &IoBus,
        scroll: &Scroll,
        font_height: u16,
        color: u8,
    ) {
        let access = self.mode.planar.then(|| PlanarAccess::new(io));
        let width = usize::from(scroll.width) * if self.mode.planar { 1 } else { 8 };
        let mut row_bytes = [0; 320]; // Largest indexed scanline in the supported BIOS layouts.
        for (row, source) in scroll.rows() {
            for scanline in 0..font_height {
                let destination = self.address(scroll.left * 8, row * font_height + scanline);
                let source =
                    source.map(|row| self.address(scroll.left * 8, row * font_height + scanline));
                if let Some(access) = &access {
                    match source {
                        Some(source) => access.copy(memory, source, destination, width),
                        None => access.fill(memory, destination, width, color),
                    }
                } else {
                    let bytes = &mut row_bytes[..width];
                    match source {
                        Some(source) => memory.read(source, bytes).expect("graphics scroll source"),
                        None => bytes.fill(color),
                    }
                    memory
                        .write(destination, bytes)
                        .expect("graphics scroll destination");
                }
            }
        }
    }
}
