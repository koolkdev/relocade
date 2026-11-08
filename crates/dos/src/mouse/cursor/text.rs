//! Software text masks and the original cell on its original BIOS page.

use super::Point;
use relocade_bios::video::{self, CharacterCell, TextPage};
use relocade_machine::MemoryView;

pub(super) struct Shape {
    pub screen: u16,
    pub cursor: u16,
}

pub(super) struct Background {
    page: TextPage,
    row: u16,
    column: u16,
    cell: CharacterCell,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            screen: 0xffff,
            cursor: 0x7700,
        }
    }
}

impl Shape {
    pub fn draw(&self, memory: &mut MemoryView<'_>, position: Point) -> Option<Background> {
        let page = TextPage::locate(memory, video::mode(memory).page)?;
        let column = position.x / if page.columns() == 40 { 16 } else { 8 };
        let row = position.y / 8;
        let cell = page.read_cell(memory, row, column)?;
        let word = u16::from_le_bytes([cell.character, cell.attribute]);
        let [character, attribute] = ((word & self.screen) ^ self.cursor).to_le_bytes();
        page.write_cell(
            memory,
            row,
            column,
            CharacterCell {
                character,
                attribute,
            },
        );
        Some(Background {
            page,
            row,
            column,
            cell,
        })
    }
}

impl Background {
    pub fn restore(self, memory: &mut MemoryView<'_>) {
        // Restore the original page even if it is no longer active. Layout equality
        // cannot detect a same-mode clear: guests must hide before changing modes.
        if self.page.matches_layout(memory) {
            self.page
                .write_cell(memory, self.row, self.column, self.cell);
        }
    }
}
