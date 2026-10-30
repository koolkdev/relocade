//! BIOS character writes and the teletype shared by INT 10h and DOS console output.
use super::{
    active_page, cursor_position, geometry::Geometry, is_text_mode, scroll, set_cursor_position,
    valid_page, ScrollDirection, Window,
};
use crate::video::{graphics::Page, Glyph};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::TEXT_MEMORY_START;

/// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
/// Text cells retain their attributes; graphics glyphs use foreground color 7.
/// This updates guest video memory and BIOS cursor data without a host output stream.
pub fn write_text(memory: &mut MemoryView<'_>, io: &IoBus, bytes: &[u8]) {
    for &byte in bytes {
        teletype(memory, io, byte, 7);
    }
}

pub(crate) fn read_cell(memory: &MemoryView<'_>, page: u8) -> [u8; 2] {
    let geometry = Geometry::read(memory);
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let mut cell = [0; 2];
    if geometry.contains(u16::from(row), u16::from(column)) {
        memory
            .read(
                geometry.cell_address(page, u16::from(row), u16::from(column)),
                &mut cell,
            )
            .expect("text cell");
    }
    cell
}

pub(crate) fn write_repeated(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    character: u8,
    color: u8,
    count: u16,
    with_attribute: bool,
) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let (column, row) = (u16::from(column), u16::from(row));
    let geometry = Geometry::read(memory);
    if is_text_mode(memory) {
        let start = geometry.cell_offset(page, row, column);
        // Text repeat writes advance a 16-bit video offset, including across
        // page padding. They never move the cursor.
        for index in 0..count {
            let offset = start.wrapping_add(index.wrapping_mul(2));
            let cell = [character, color];
            memory
                .write(
                    TEXT_MEMORY_START + u32::from(offset),
                    &cell[..if with_attribute { 2 } else { 1 }],
                )
                .expect("text cell");
        }
    } else if geometry.contains(row, column) && count != 0 {
        let Some(page) = Page::locate(memory, page) else {
            return;
        };
        let Some(glyph) = Glyph::read(memory, character, geometry.font_height) else {
            return;
        };
        // Cross-row repeats are unspecified by the BIOS ABI. Our graphics
        // policy wraps through visible cells and stops at the page bottom.
        let start = row * geometry.columns + column;
        let end = start
            .saturating_add(count)
            .min(geometry.columns * geometry.rows);
        let positions =
            (start..end).map(|index| (index / geometry.columns, index % geometry.columns));
        page.write_glyphs(memory, io, positions, glyph.rows(), color);
    }
}

pub(crate) fn teletype(memory: &mut MemoryView<'_>, io: &IoBus, character: u8, color: u8) {
    let page = active_page(memory);
    if !valid_page(memory, page) {
        return;
    }
    let geometry = Geometry::read(memory);
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let (mut column, mut row) = (u16::from(column), u16::from(row));
    if !geometry.contains(row, column) {
        return;
    }
    match character {
        7 => return, // Bell requires a future speaker device.
        8 => column = column.saturating_sub(1),
        b'\r' => column = 0,
        b'\n' => row += 1,
        _ => {
            write_repeated(memory, io, page, character, color, 1, false);
            column += 1;
            if column == geometry.columns {
                column = 0;
                row += 1;
            }
        }
    }
    if row == geometry.rows {
        row -= 1;
        // Text scrolling takes the attribute after cursor advance; graphics
        // scrolling always exposes black pixels, regardless of the foreground.
        set_cursor_position(
            memory,
            io,
            page,
            u16::from_le_bytes([column as u8, row as u8]),
        );
        let fill = if is_text_mode(memory) {
            read_cell(memory, page)[1]
        } else {
            0
        };
        scroll(
            memory,
            io,
            Window {
                top: 0,
                left: 0,
                bottom: row as u8,
                right: (geometry.columns - 1).min(255) as u8,
            },
            1,
            fill,
            ScrollDirection::Up,
        );
    }
    set_cursor_position(
        memory,
        io,
        page,
        u16::from_le_bytes([column as u8, row as u8]),
    );
}
