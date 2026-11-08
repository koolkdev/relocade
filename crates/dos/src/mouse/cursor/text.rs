//! Software text masks and the original guest-visible character/attribute word.
use relocade_vga::{Framebuffer, TextCell};

pub(in crate::mouse) struct Shape {
    pub screen: u16,
    pub cursor: u16,
}

pub(super) struct Background(TextCell);

impl Default for Shape {
    fn default() -> Self {
        Self {
            screen: 0xffff,
            cursor: 0x7700,
        }
    }
}

impl Shape {
    pub(super) fn draw(
        &self,
        frame: &mut Framebuffer<'_>,
        position: [i32; 2],
    ) -> Option<Background> {
        let [column, row] = position;
        let cell = frame.text_cell(row.try_into().ok()?, column.try_into().ok()?)?;
        frame.write_text_cell(&cell, (cell.value() & self.screen) ^ self.cursor);
        Some(Background(cell))
    }
}

impl Background {
    pub fn restore(self, frame: &mut Framebuffer<'_>) {
        frame.write_text_cell(&self.0, self.0.value());
    }
}
