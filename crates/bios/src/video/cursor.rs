//! BIOS cursor state and active page selection.

use super::CURSOR_SHAPE;
use crate::screen;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};

/// Stored cursor position and requested shape, before VGA shape emulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorState {
    pub row: u8,
    pub column: u8,
    /// High byte: start scanline/control bits. Low byte: end scanline/skew bits.
    pub shape: u16,
}

/// Queries a text or graphics page's cursor. Invalid pages return None.
#[interrupt(context(memory), args(page = bh), returns {
    Some(cursor) => { cx = cursor.shape, dh = cursor.row, dl = cursor.column },
    None => crate::interrupt::unsupported,
})]
pub fn cursor(memory: &MemoryView<'_>, page: u8) -> Option<CursorState> {
    if !screen::valid_page(memory, page) {
        return None;
    }
    let [column, row] = screen::cursor_position(memory, page).to_le_bytes();
    Some(CursorState {
        row,
        column,
        shape: memory.read_u16(CURSOR_SHAPE).expect("video cursor shape"),
    })
}

/// Sets a page's stored cursor. Only the active text page updates VGA's cursor.
/// Coordinates remain guest values, even outside the visible grid.
#[interrupt(
    context(memory, io), args(page = bh, row = dh, column = dl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn set_cursor(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    row: u8,
    column: u8,
) -> Option<()> {
    if !screen::valid_page(memory, page) {
        return None;
    }
    screen::set_cursor_position(memory, io, page, u16::from_le_bytes([column, row]));
    Some(())
}

/// Sets the requested CX-format cursor shape, applying BIOS emulation policy.
/// Graphics modes return None without changing the stored shape.
#[interrupt(
    context(memory, io), args(shape = cx),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn set_cursor_shape(memory: &mut MemoryView<'_>, io: &IoBus, shape: u16) -> Option<()> {
    if !screen::is_text_mode(memory) {
        return None;
    }
    screen::set_cursor_shape(memory, io, shape);
    Some(())
}

/// Selects a valid text or graphics page, preserving each page's stored cursor.
#[interrupt(
    context(memory, io), args(page = al),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn select_page(memory: &mut MemoryView<'_>, io: &IoBus, page: u8) -> Option<()> {
    if !screen::valid_page(memory, page) {
        return None;
    }
    screen::select_page(memory, io, page);
    Some(())
}
