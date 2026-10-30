//! BIOS graphics layouts and native scanline operations through the VGA aperture.
mod access;
mod glyph;
mod layout;
mod pixel;
pub(super) use pixel::Pixel;

use crate::screen::Scroll;
use access::GraphicsAccess;
use layout::{Layout, Mode};
use relocade_machine::{IoBus, MemoryView};

impl Mode {
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
        self.offset / self.mode.layout.start_divisor()
    }

    pub fn columns(&self) -> u16 {
        self.mode.width / 8
    }

    pub fn rows(&self, font_height: u16) -> u16 {
        self.mode.height.checked_div(font_height).unwrap_or(0)
    }

    fn address(&self, x: u16, y: u16) -> u32 {
        let layout = self.mode.layout;
        let pixels_per_byte = layout.pixels_per_byte();
        layout.aperture().start
            + u32::from(self.offset)
            + layout.row_offset(self.mode.width / pixels_per_byte, y)
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
        let layout = self.mode.layout;
        let access = GraphicsAccess::new(io, layout.write_planes());
        let width = usize::from(scroll.width * 8 / layout.pixels_per_byte());
        let mut row_bytes = [0; 320]; // Largest indexed scanline in the supported BIOS layouts.
        for (row, source) in scroll.rows() {
            for scanline in 0..font_height {
                let destination = self.address(scroll.left * 8, row * font_height + scanline);
                let source =
                    source.map(|row| self.address(scroll.left * 8, row * font_height + scanline));
                if layout == Layout::Planar16 {
                    match source {
                        Some(source) => access.copy_planar(memory, source, destination, width),
                        None => access.fill_planar(memory, destination, width, color),
                    }
                } else {
                    let bytes = &mut row_bytes[..width];
                    match source {
                        Some(source) => memory.read(source, bytes).expect("graphics scroll source"),
                        // IBM VGA packed modes repeat BH as a byte pattern.
                        // Indexed modes also use the whole byte as their fill.
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

/// Mode setting clears the selected aperture through its newly restored routing.
pub(super) fn clear(memory: &mut MemoryView<'_>) {
    let mode = Mode::read(memory).expect("graphics mode preset");
    let blank = [0; 4096];
    for address in mode.layout.aperture().step_by(blank.len()) {
        memory.write(address, &blank).expect("graphics memory");
    }
}
