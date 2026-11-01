//! Validates font-height transitions before either font memory or geometry changes.
use super::super::{
    crtc_port, modes, read_indexed, write_indexed, CHARACTER_HEIGHT, COLUMNS, LAST_ROW, MODE,
    PAGE_SIZE,
};
use crate::screen;
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{registers::crtc, TEXT_MEMORY_BYTES};

pub(super) struct Layout {
    height: u8,
    rows: u16,
    page_bytes: u16,
    crtc_port: u16,
}

impl Layout {
    pub fn for_font(memory: &MemoryView<'_>, io: &IoBus, height: u8) -> Option<Self> {
        // Geometry-changing font services require the caller to select page zero.
        if screen::active_page(memory) != 0 {
            return None;
        }
        let crtc_port = crtc_port(io);
        let index = io.read_port(crtc_port, 1);
        let end = u16::from(read_indexed(io, crtc_port, crtc::VERTICAL_DISPLAY_END));
        let overflow = u16::from(read_indexed(io, crtc_port, crtc::OVERFLOW));
        let mode = read_indexed(io, crtc_port, crtc::MODE_CONTROL);
        io.write_port(crtc_port, 1, index);
        let lines = (end | ((overflow & 2) << 7) | ((overflow & 0x40) << 3)) + 1;
        let lines = lines << u16::from(mode & 4 != 0);
        let rows = lines / u16::from(height);
        let columns = memory.read_u16(COLUMNS).expect("video columns");
        // Firmware allocates complete 2-KiB blocks. The result must fit one
        // text page and the BIOS's byte-sized last-row field.
        let page_bytes = (u32::from(rows) * u32::from(columns) * 2).next_multiple_of(2048);
        if !(1..=256).contains(&rows) || page_bytes == 0 || page_bytes > TEXT_MEMORY_BYTES {
            return None;
        }
        Some(Self {
            height,
            rows,
            page_bytes: page_bytes as u16,
            crtc_port,
        })
    }

    pub fn apply(self, memory: &mut MemoryView<'_>, io: &IoBus) {
        if memory.read_u8(MODE).expect("video mode") == modes::MONO_TEXT_80X25 {
            write_indexed(
                io,
                self.crtc_port,
                crtc::UNDERLINE_LOCATION,
                self.height - 1,
            );
        }
        let scan_line = read_indexed(io, self.crtc_port, crtc::MAXIMUM_SCAN_LINE);
        // Keep overflow bits; clear double scanning so each font row gets one
        // scanline. Timing and VDE stay intact, including any partial final row.
        write_indexed(
            io,
            self.crtc_port,
            crtc::MAXIMUM_SCAN_LINE,
            (scan_line & 0x60) | (self.height - 1),
        );
        memory
            .write_u8(LAST_ROW, (self.rows - 1) as u8)
            .expect("video rows");
        memory
            .write_u16(CHARACTER_HEIGHT, u16::from(self.height))
            .expect("font height");
        memory
            .write_u16(PAGE_SIZE, self.page_bytes)
            .expect("video page size");
        let shape = u16::from_be_bytes([self.height.saturating_sub(2), self.height - 1]);
        screen::set_cursor_shape(memory, io, shape);
    }
}
