//! BIOS text-page operations and firmware display data.
mod geometry;
use geometry::Geometry;

use crate::video::{
    write_crtc_word, ACTIVE_PAGE, COLOR_TEXT_80X25, CURSORS, CURSOR_SHAPE, MODE, PAGE_OFFSET,
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{registers::crtc, TEXT_MEMORY_START};

/// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
/// CR, LF and backspace move the cursor; other glyphs retain cell attributes.
/// This updates guest video memory and BIOS cursor data without a host output stream.
/// Output in graphics modes is currently ignored.
pub fn write_text(memory: &mut MemoryView<'_>, io: &IoBus, bytes: &[u8]) {
    if !is_text_mode(memory) {
        return;
    }
    let page = active_page(memory);
    for &byte in bytes {
        teletype(memory, io, page, byte);
    }
}

pub(super) fn is_text_mode(memory: &MemoryView<'_>) -> bool {
    memory.read_u8(MODE).expect("video mode") == COLOR_TEXT_80X25
}

pub(super) fn active_page(memory: &MemoryView<'_>) -> u8 {
    memory.read_u8(ACTIVE_PAGE).expect("active video page")
}

pub(super) fn valid_page(memory: &MemoryView<'_>, page: u8) -> bool {
    Geometry::read(memory).valid_page(page)
}

pub(super) fn select_page(memory: &mut MemoryView<'_>, io: &IoBus, page: u8) {
    let offset = Geometry::read(memory).page_offset(page);
    memory
        .write_u8(ACTIVE_PAGE, page)
        .expect("active video page");
    memory
        .write_u16(PAGE_OFFSET, offset)
        .expect("video page offset");
    write_crtc_word(io, crtc::START_ADDRESS_HIGH, offset / 2);
    update_cursor(memory, io, page);
}

pub(super) fn cursor_position(memory: &MemoryView<'_>, page: u8) -> u16 {
    memory
        .read_u16(CURSORS + u32::from(page) * 2)
        .expect("video cursor")
}

pub(super) fn set_cursor_position(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    position: u16,
) {
    memory
        .write_u16(CURSORS + u32::from(page) * 2, position)
        .expect("video cursor");
    if page == active_page(memory) {
        update_cursor(memory, io, page);
    }
}

pub(super) fn set_cursor_shape(memory: &mut MemoryView<'_>, io: &IoBus, shape: u16) {
    memory
        .write_u16(CURSOR_SHAPE, shape)
        .expect("video cursor shape");
    write_crtc_word(io, crtc::CURSOR_START, shape);
}

fn update_cursor(memory: &MemoryView<'_>, io: &IoBus, page: u8) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let position = Geometry::read(memory).cursor_address(page, u16::from(row), u16::from(column));
    write_crtc_word(io, crtc::CURSOR_ADDRESS_HIGH, position);
}

pub(super) fn read_cell(memory: &MemoryView<'_>, page: u8) -> [u8; 2] {
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

pub(super) fn write_repeated(
    memory: &mut MemoryView<'_>,
    page: u8,
    character: u8,
    attribute: Option<u8>,
    count: u16,
) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let start = Geometry::read(memory).cell_offset(page, u16::from(row), u16::from(column));
    // BIOS repeat writes advance a 16-bit video offset; the visible page edge
    // does not stop them or move the cursor.
    for index in 0..count {
        let offset = start.wrapping_add(index.wrapping_mul(2));
        let address = TEXT_MEMORY_START + u32::from(offset);
        let cell = [character, attribute.unwrap_or(0)];
        memory
            .write(address, &cell[..if attribute.is_some() { 2 } else { 1 }])
            .expect("text cell");
    }
}

pub(super) struct Window {
    pub top: u8,
    pub left: u8,
    pub bottom: u8,
    pub right: u8,
}

pub(super) enum ScrollDirection {
    Up,
    Down,
}

pub(super) fn scroll(
    memory: &mut MemoryView<'_>,
    page: u8,
    window: Window,
    lines: u8,
    attribute: u8,
    direction: ScrollDirection,
) {
    let geometry = Geometry::read(memory);
    if geometry.columns == 0 {
        return;
    }
    let bottom = u16::from(window.bottom).min(geometry.rows - 1);
    let right = u16::from(window.right).min(geometry.columns - 1);
    let top = u16::from(window.top);
    let left = u16::from(window.left);
    if top > bottom || left > right {
        return;
    }
    let height = bottom - top + 1;
    let count = if lines == 0 {
        height
    } else {
        u16::from(lines).min(height)
    };
    let width_bytes = usize::from(right - left + 1) * 2;
    // INT 10h window columns are bytes, so at most 256 cells can be selected.
    let mut row_bytes = [0; 256 * 2];
    for index in 0..height {
        let (row, source) = match direction {
            ScrollDirection::Up => {
                let row = top + index;
                (row, i32::from(row) + i32::from(count))
            }
            ScrollDirection::Down => {
                let row = bottom - index;
                (row, i32::from(row) - i32::from(count))
            }
        };
        let bytes = &mut row_bytes[..width_bytes];
        if source >= i32::from(top) && source <= i32::from(bottom) {
            memory
                .read(geometry.cell_address(page, source as u16, left), bytes)
                .expect("scroll source");
        } else {
            for cell in bytes.as_chunks_mut::<2>().0 {
                *cell = [b' ', attribute];
            }
        }
        memory
            .write(geometry.cell_address(page, row, left), bytes)
            .expect("scroll destination");
    }
}

fn teletype(memory: &mut MemoryView<'_>, io: &IoBus, page: u8, character: u8) {
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
            memory
                .write_u8(geometry.cell_address(page, row, column), character)
                .expect("text character");
            column += 1;
            if column == geometry.columns {
                column = 0;
                row += 1;
            }
        }
    }
    if row == geometry.rows {
        row -= 1;
        // BIOS reads the fill attribute after advancing the cursor. A wrapped
        // line uses column zero; a line feed retains its previous column.
        set_cursor_position(
            memory,
            io,
            page,
            u16::from_le_bytes([column as u8, row as u8]),
        );
        let attribute = read_cell(memory, page)[1];
        scroll(
            memory,
            page,
            Window {
                top: 0,
                left: 0,
                bottom: row as u8,
                right: (geometry.columns - 1).min(255) as u8,
            },
            1,
            attribute,
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
