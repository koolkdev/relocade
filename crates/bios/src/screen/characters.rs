//! BIOS character writes and the output cursor shared by INT 10h and DOS.
use super::{
    active_page, cursor_position, geometry::Geometry, is_text_mode, scroll, set_cursor_position,
    valid_page, ScrollDirection, Window,
};
use crate::video::{buffer_addresses, graphics::Page, Glyph, GRAPHICS_640X480_2, MODE};
use relocade_machine::{IoBus, MemoryView};

/// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
/// Text cells retain their attributes; graphics glyphs use foreground color 7.
/// This updates guest video memory and BIOS cursor data without a host output stream.
pub fn write_text(memory: &mut MemoryView<'_>, io: &IoBus, bytes: &[u8]) {
    let page = active_page(memory);
    if let Some(mut output) = Output::new(memory, io, page) {
        for &byte in bytes {
            output.teletype(byte, 7);
        }
        output.commit_cursor();
    }
}

pub(crate) fn read_cell(memory: &MemoryView<'_>, page: u8) -> [u8; 2] {
    let geometry = Geometry::read(memory);
    let position = CursorPosition::from_bios(cursor_position(memory, page));
    read_cell_at(memory, &geometry, page, position)
}

fn read_cell_at(
    memory: &MemoryView<'_>,
    geometry: &Geometry,
    page: u8,
    position: CursorPosition,
) -> [u8; 2] {
    let mut cell = [0; 2];
    if geometry.contains(position.row, position.column) {
        memory
            .read(
                geometry.cell_address(page, position.row, position.column),
                &mut cell,
            )
            .expect("text cell");
    }
    cell
}

#[derive(Clone, Copy)]
struct CursorPosition {
    row: u16,
    column: u16,
}

impl CursorPosition {
    fn from_bios(position: u16) -> Self {
        let [column, row] = position.to_le_bytes();
        Self {
            row: u16::from(row),
            column: u16::from(column),
        }
    }

    fn to_bios(self) -> u16 {
        u16::from_le_bytes([self.column as u8, self.row as u8])
    }
}

/// Writes to one BIOS page using a local cursor. Only `commit_cursor` publishes
/// that position; drawing and scrolling never switch the active display page.
pub(crate) struct Output<'a, 'memory> {
    memory: &'a mut MemoryView<'memory>,
    io: &'a IoBus,
    page: u8,
    geometry: Geometry,
    // None uses the stored BIOS cursor and leaves it untouched on commit.
    cursor: Option<CursorPosition>,
}

impl<'a, 'memory> Output<'a, 'memory> {
    pub fn new(memory: &'a mut MemoryView<'memory>, io: &'a IoBus, page: u8) -> Option<Self> {
        if !valid_page(memory, page) {
            return None;
        }
        let geometry = Geometry::read(memory);
        Some(Self {
            memory,
            io,
            page,
            geometry,
            cursor: None,
        })
    }

    pub fn set_position(&mut self, position: u16) {
        self.cursor = Some(CursorPosition::from_bios(position));
    }

    pub fn commit_cursor(self) {
        if let Some(position) = self.cursor {
            set_cursor_position(self.memory, self.io, self.page, position.to_bios());
        }
    }

    fn position(&self) -> CursorPosition {
        self.cursor
            .unwrap_or_else(|| CursorPosition::from_bios(cursor_position(self.memory, self.page)))
    }

    pub fn write_repeated(&mut self, character: u8, color: u8, count: u16, with_attribute: bool) {
        let position = self.position();
        if is_text_mode(self.memory) {
            let start = self
                .geometry
                .cell_offset(self.page, position.row, position.column);
            // Text repeats advance a 16-bit video offset, including across
            // page padding. They never move the cursor.
            for index in 0..count {
                let offset = start.wrapping_add(index.wrapping_mul(2));
                let cell = [character, color];
                self.memory
                    .write(
                        self.geometry.text_address(offset),
                        &cell[..if with_attribute { 2 } else { 1 }],
                    )
                    .expect("text cell");
            }
        } else if self.geometry.contains(position.row, position.column) && count != 0 {
            let Some(page) = Page::locate(self.memory, self.page) else {
                return;
            };
            let Some(glyph) = Glyph::read(self.memory, character, self.geometry.font_height) else {
                return;
            };
            // AH=09h and string writes force white in IBM mode 11h, retaining
            // the XOR flag. AH=0Ah and teletype use the caller's color bits.
            let color = if with_attribute
                && self.memory.read_u8(MODE).expect("video mode") == GRAPHICS_640X480_2
            {
                color | 15
            } else {
                color
            };
            // Cross-row repeats are unspecified by the BIOS ABI. Our graphics
            // policy wraps through visible cells and stops at the page bottom.
            let start = position.row * self.geometry.columns + position.column;
            let end = start
                .saturating_add(count)
                .min(self.geometry.columns * self.geometry.rows);
            let positions = (start..end)
                .map(|index| (index / self.geometry.columns, index % self.geometry.columns));
            page.write_glyphs(self.memory, self.io, positions, glyph.rows(), color);
        }
    }

    pub fn teletype(&mut self, character: u8, color: u8) {
        self.write_character(character, color, false);
    }

    /// `None` reads a character/attribute pair for every character, including
    /// control characters. Reads stay live when the source aliases video memory.
    pub fn write_string(&mut self, base: u32, offset: u16, count: u16, attribute: Option<u8>) {
        let bytes_per_character = if attribute.is_some() { 1 } else { 2 };
        let mut addresses = buffer_addresses(base, offset, u32::from(count) * bytes_per_character);
        for _ in 0..count {
            let character = self
                .memory
                .read_u8(addresses.next().expect("string character address"))
                .expect("string character");
            let color = attribute.unwrap_or_else(|| {
                self.memory
                    .read_u8(addresses.next().expect("string attribute address"))
                    .expect("string attribute")
            });
            self.write_character(character, color, true);
        }
    }

    fn write_character(&mut self, character: u8, color: u8, with_attribute: bool) {
        let mut position = self.position();
        if !self.geometry.contains(position.row, position.column) {
            return;
        }
        match character {
            7 => return, // Bell requires a future speaker device.
            8 => position.column = position.column.saturating_sub(1),
            b'\r' => position.column = 0,
            b'\n' => position.row += 1,
            _ => {
                self.write_repeated(character, color, 1, with_attribute);
                position.column += 1;
                if position.column == self.geometry.columns {
                    position.column = 0;
                    position.row += 1;
                }
            }
        }
        if position.row == self.geometry.rows {
            position.row -= 1;
            // Text scrolling takes the attribute after cursor advance; graphics
            // scrolling always exposes black pixels, regardless of the foreground.
            let fill = if is_text_mode(self.memory) {
                read_cell_at(self.memory, &self.geometry, self.page, position)[1]
            } else {
                0
            };
            scroll(
                self.memory,
                self.io,
                self.page,
                Window {
                    top: 0,
                    left: 0,
                    bottom: position.row as u8,
                    right: (self.geometry.columns - 1).min(255) as u8,
                },
                1,
                fill,
                ScrollDirection::Up,
            );
        }
        self.cursor = Some(position);
    }
}
