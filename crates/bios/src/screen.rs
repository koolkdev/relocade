//! BIOS character grids, pages, cursors and firmware display data.
mod characters;
mod cursor;
mod geometry;
mod scroll;

use crate::video::graphics::Page as GraphicsPage;
pub(super) use characters::read_cell;
pub use characters::{write_text, CursorPosition, Output};
pub(super) use cursor::{
    cursor_position, set_cursor_emulation, set_cursor_position, set_cursor_shape,
    set_native_cursor_shape,
};
use geometry::Geometry;
pub(super) use scroll::{Scroll, ScrollDirection, Window};

use crate::video::{text_memory_start, write_crtc_word, ACTIVE_PAGE, MODE, PAGE_OFFSET};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::crtc;

pub(super) fn is_text_mode(memory: &MemoryView<'_>) -> bool {
    text_memory_start(memory.read_u8(MODE).expect("video mode")).is_some()
}

pub(super) fn active_page(memory: &MemoryView<'_>) -> u8 {
    memory.read_u8(ACTIVE_PAGE).expect("active video page")
}

pub(super) fn valid_page(memory: &MemoryView<'_>, page: u8) -> bool {
    if is_text_mode(memory) {
        Geometry::read(memory).valid_page(page)
    } else {
        GraphicsPage::locate(memory, page).is_some()
    }
}

pub(super) fn select_page(memory: &mut MemoryView<'_>, io: &IoBus, page: u8) {
    let (offset, start) = if is_text_mode(memory) {
        let offset = Geometry::read(memory).page_offset(page);
        (offset, offset / 2)
    } else {
        let page = GraphicsPage::locate(memory, page).expect("validated graphics page");
        (page.offset(), page.display_start())
    };
    memory
        .write_u8(ACTIVE_PAGE, page)
        .expect("active video page");
    memory
        .write_u16(PAGE_OFFSET, offset)
        .expect("video page offset");
    write_crtc_word(io, crtc::START_ADDRESS_HIGH, start);
    cursor::update_cursor(memory, io, page);
}

pub(super) fn scroll(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    window: Window,
    lines: u8,
    color: u8,
    direction: ScrollDirection,
) {
    let geometry = Geometry::read(memory);
    let Some(scroll) = Scroll::new(window, lines, direction, geometry.columns, geometry.rows)
    else {
        return;
    };
    if is_text_mode(memory) {
        scroll.text(memory, &geometry, page, color);
    } else if let Some(page) = GraphicsPage::locate(memory, page) {
        page.scroll(memory, io, &scroll, geometry.font_height, color);
    }
}
