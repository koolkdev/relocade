//! Explicit-position text access shares the BIOS geometry and mapped memory path.

use super::{is_text_mode, Geometry};
use crate::video::CharacterCell;
use relocade_machine::MemoryView;

/// One BIOS text page's address layout, independent of the BIOS output cursor.
/// The layout must still match when accessing cells. Equality describes addresses
/// and geometry; it cannot tell whether a mode set cleared the page's contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextPage {
    geometry: Geometry,
    page: u8,
}

impl TextPage {
    pub fn locate(memory: &MemoryView<'_>, page: u8) -> Option<Self> {
        let geometry = Geometry::read(memory);
        (is_text_mode(memory) && geometry.valid_page(page)).then_some(Self { geometry, page })
    }

    pub fn columns(&self) -> u16 {
        self.geometry.columns
    }

    pub fn rows(&self) -> u16 {
        self.geometry.rows
    }

    /// Checks the recorded page layout, even if a different page is now active.
    pub fn matches_layout(&self, memory: &MemoryView<'_>) -> bool {
        Self::locate(memory, self.page).as_ref() == Some(self)
    }

    pub fn read_cell(
        &self,
        memory: &MemoryView<'_>,
        row: u16,
        column: u16,
    ) -> Option<CharacterCell> {
        let [character, attribute] = self.geometry.read_cell(memory, self.page, row, column)?;
        Some(CharacterCell {
            character,
            attribute,
        })
    }

    pub fn write_cell(
        &self,
        memory: &mut MemoryView<'_>,
        row: u16,
        column: u16,
        cell: CharacterCell,
    ) -> Option<()> {
        self.geometry.write_cell(
            memory,
            self.page,
            row,
            column,
            [cell.character, cell.attribute],
        )
    }
}
