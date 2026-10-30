//! BIOS character grids and text addressing use firmware bookkeeping.
use crate::video::{graphics::Page, CHARACTER_HEIGHT, COLUMNS, LAST_ROW, PAGE_SIZE};
use relocade_machine::MemoryView;
use relocade_vga::{TEXT_MEMORY_BYTES, TEXT_MEMORY_START};

pub(super) struct Geometry {
    pub columns: u16,
    pub rows: u16,
    pub font_height: u16,
    page_bytes: u16,
}

impl Geometry {
    pub fn read(memory: &MemoryView<'_>) -> Self {
        let mut geometry = Self {
            columns: memory.read_u16(COLUMNS).expect("video columns"),
            rows: u16::from(memory.read_u8(LAST_ROW).expect("video rows")) + 1,
            font_height: memory.read_u16(CHARACTER_HEIGHT).expect("font height"),
            page_bytes: memory.read_u16(PAGE_SIZE).expect("video page size"),
        };
        if let Some(page) = Page::locate(memory, 0) {
            geometry.columns = geometry.columns.min(page.columns());
            geometry.rows = geometry.rows.min(page.rows(geometry.font_height));
        }
        geometry
    }

    pub fn valid_page(&self, page: u8) -> bool {
        // There are eight BIOS cursor records and 32 KiB of text memory.
        page < 8
            && self.page_bytes != 0
            && (u32::from(page) + 1) * u32::from(self.page_bytes) <= TEXT_MEMORY_BYTES
    }

    pub fn contains(&self, row: u16, column: u16) -> bool {
        row < self.rows && column < self.columns
    }

    pub fn page_offset(&self, page: u8) -> u16 {
        u16::from(page).wrapping_mul(self.page_bytes)
    }

    pub fn cursor_address(&self, page: u8, row: u16, column: u16) -> u16 {
        (self.page_offset(page) / 2).wrapping_add(self.cell_index(row, column))
    }

    pub fn cell_offset(&self, page: u8, row: u16, column: u16) -> u16 {
        self.page_offset(page)
            .wrapping_add(self.cell_index(row, column).wrapping_mul(2))
    }

    pub fn cell_address(&self, page: u8, row: u16, column: u16) -> u32 {
        TEXT_MEMORY_START + u32::from(self.cell_offset(page, row, column))
    }

    fn cell_index(&self, row: u16, column: u16) -> u16 {
        row.wrapping_mul(self.columns).wrapping_add(column)
    }
}
