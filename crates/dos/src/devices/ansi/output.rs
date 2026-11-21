//! ANSI text output composed from the public BIOS video services.

use relocade_bios::video::{self, CharacterRun, Window};
use relocade_machine::{IoBus, MemoryView};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct CursorPosition {
    pub row: u16,
    pub column: u16,
}

pub(super) struct TextOutput<'a, 'memory> {
    memory: &'a mut MemoryView<'memory>,
    io: &'a IoBus,
    page: u8,
    window: Window,
}

impl<'a, 'memory> TextOutput<'a, 'memory> {
    pub fn new(memory: &'a mut MemoryView<'memory>, io: &'a IoBus) -> Option<Self> {
        let window = video::text_window(memory)?;
        let page = video::mode(memory).page;
        Some(Self {
            memory,
            io,
            page,
            window,
        })
    }

    pub fn position(&self) -> CursorPosition {
        let cursor = video::cursor(self.memory, self.page).expect("active text page");
        CursorPosition {
            row: u16::from(cursor.row),
            column: u16::from(cursor.column),
        }
    }

    pub fn rows(&self) -> u16 {
        u16::from(self.window.bottom) + 1
    }

    pub fn columns(&self) -> u16 {
        u16::from(self.window.right) + 1
    }

    pub fn move_to(&mut self, position: CursorPosition) {
        video::set_cursor(
            self.memory,
            self.io,
            self.page,
            position.row.min(u16::from(self.window.bottom)) as u8,
            position.column.min(u16::from(self.window.right)) as u8,
        )
        .expect("active text page");
    }

    pub fn write(&mut self, bytes: &[u8], attribute: u8) {
        for &byte in bytes {
            let mut cursor = self.position();
            if cursor.row >= self.rows() || cursor.column >= self.columns() {
                continue;
            }
            match byte {
                7 => continue, // Bell requires a future speaker device.
                8 => cursor.column = cursor.column.saturating_sub(1),
                b'\r' => cursor.column = 0,
                b'\n' => cursor.row += 1,
                _ => {
                    video::write_characters_with_attribute(
                        self.memory,
                        self.io,
                        CharacterRun {
                            page: self.page,
                            character: byte,
                            color: attribute,
                            count: 1,
                        },
                    )
                    .expect("active text page");
                    cursor.column += 1;
                    if cursor.column == self.columns() {
                        cursor.column = 0;
                        cursor.row += 1;
                    }
                }
            }
            if cursor.row == self.rows() {
                cursor.row -= 1;
                // ANSI fills newly exposed rows with its current rendition.
                video::scroll_up(self.memory, self.io, self.window, 1, attribute);
            }
            self.move_to(cursor);
        }
    }

    pub fn clear(&mut self, start: CursorPosition, end: CursorPosition, attribute: u8) {
        if start.row > 255 || start.column > 255 {
            return;
        }
        video::scroll_up(
            self.memory,
            self.io,
            Window {
                top: start.row as u8,
                left: start.column as u8,
                bottom: end.row.min(255) as u8,
                right: end.column.min(255) as u8,
            },
            0,
            attribute,
        );
    }
}
