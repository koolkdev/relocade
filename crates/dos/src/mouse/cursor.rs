//! Cursor visibility, shape and restoration of the guest's video memory.

use super::state::Point;
use relocade_bios::video::{self, CharacterCell, TextPage};
use relocade_machine::MemoryView;

pub(super) struct Cursor {
    visibility: i16,
    screen_mask: u16,
    cursor_mask: u16,
    saved: Option<SavedText>,
}

struct SavedText {
    page: TextPage,
    row: u16,
    column: u16,
    cell: CharacterCell,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            visibility: -1,
            screen_mask: 0xffff,
            cursor_mask: 0x7700,
            saved: None,
        }
    }
}

impl Cursor {
    pub fn reset(&mut self, memory: &mut MemoryView<'_>) {
        self.restore(memory);
        *self = Self::default();
    }

    pub fn visibility(&mut self, show: bool) {
        self.visibility = if show {
            (self.visibility + 1).min(0)
        } else {
            self.visibility.saturating_sub(1)
        };
    }

    pub fn set_text_mask(&mut self, screen: u16, cursor: u16) {
        self.screen_mask = screen;
        self.cursor_mask = cursor;
    }

    pub fn update(&mut self, memory: &mut MemoryView<'_>, position: Point) {
        self.restore(memory);
        if self.visibility != 0 {
            return;
        }
        let mode = video::mode(memory);
        let Some(page) = TextPage::locate(memory, mode.page) else {
            return;
        };
        let column = position.x / if page.columns() == 40 { 16 } else { 8 };
        let row = position.y / 8;
        let Some(cell) = page.read_cell(memory, row, column) else {
            return;
        };
        let word = u16::from_le_bytes([cell.character, cell.attribute]);
        let [character, attribute] = ((word & self.screen_mask) ^ self.cursor_mask).to_le_bytes();
        page.write_cell(
            memory,
            row,
            column,
            CharacterCell {
                character,
                attribute,
            },
        );
        self.saved = Some(SavedText {
            page,
            row,
            column,
            cell,
        });
    }

    fn restore(&mut self, memory: &mut MemoryView<'_>) {
        if let Some(saved) = self.saved.take() {
            // Restore the original page, not whichever page is currently active.
            // A matching layout is not evidence that a same-mode clear did not occur:
            // guests must hide before changing video modes or drawing under the cursor.
            if saved.page.matches_layout(memory) {
                saved
                    .page
                    .write_cell(memory, saved.row, saved.column, saved.cell);
            }
        }
    }
}
