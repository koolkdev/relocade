//! BIOS character grids, pages, cursors and firmware display data.
mod characters;
mod geometry;
mod scroll;

use crate::video::graphics::Page as GraphicsPage;
pub use characters::write_text;
pub(super) use characters::{read_cell, Output};
use geometry::Geometry;
pub(super) use scroll::{Scroll, ScrollDirection, Window};

use crate::video::{
    text_memory_start, write_crtc_word, ACTIVE_PAGE, CURSORS, CURSOR_SHAPE, MODE, PAGE_OFFSET,
};
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
    // Graphics cursors are BIOS bookkeeping only.
    if !is_text_mode(memory) {
        return;
    }
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let position = Geometry::read(memory).cursor_address(page, u16::from(row), u16::from(column));
    write_crtc_word(io, crtc::CURSOR_ADDRESS_HIGH, position);
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
