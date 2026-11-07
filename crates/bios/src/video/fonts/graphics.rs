//! Graphics font selection changes BIOS pointers and geometry, not VGA font memory.
use super::rom;
use crate::video::{CHARACTER_HEIGHT, LAST_ROW};
use relocade_machine::{
    real_mode_interrupt::{interrupt, write_vector, FarPointer},
    MemoryView,
};

pub(in crate::video) struct Rows {
    pub(super) selection: u8,
    pub(super) custom: u8,
}

#[interrupt(context(memory), args(source = ptr(es, bp)))]
pub(in crate::video) fn set_high_font(memory: &mut MemoryView<'_>, source: FarPointer) {
    write_vector(memory, crate::interrupts::HIGH_FONT, source);
}

#[interrupt(context(memory),
    args(source = ptr(es, bp), height = cx, rows = Rows { selection: bl, custom: dl }),
    returns { Some(()) => {}, None => crate::interrupts::unsupported },
)]
pub(in crate::video) fn set_font(
    memory: &mut MemoryView<'_>,
    source: FarPointer,
    height: u16,
    rows: Rows,
) -> Option<()> {
    select(memory, source, height, rows)
}

#[interrupt(context(memory), args(rows = Rows { selection: bl, custom: dl }),
    returns { Some(()) => {}, None => crate::interrupts::unsupported },
)]
pub(in crate::video) fn set_8x14(memory: &mut MemoryView<'_>, rows: Rows) -> Option<()> {
    select(memory, rom::for_height(14).pointer(), 14, rows)
}

#[interrupt(context(memory), args(rows = Rows { selection: bl, custom: dl }),
    returns { Some(()) => {}, None => crate::interrupts::unsupported },
)]
pub(in crate::video) fn set_8x8(memory: &mut MemoryView<'_>, rows: Rows) -> Option<()> {
    select(memory, rom::for_height(8).pointer(), 8, rows)
}

#[interrupt(context(memory), args(rows = Rows { selection: bl, custom: dl }),
    returns { Some(()) => {}, None => crate::interrupts::unsupported },
)]
pub(in crate::video) fn set_8x16(memory: &mut MemoryView<'_>, rows: Rows) -> Option<()> {
    select(memory, rom::for_height(16).pointer(), 16, rows)
}

fn select(memory: &mut MemoryView<'_>, pointer: FarPointer, height: u16, rows: Rows) -> Option<()> {
    // Keep the same supported glyph heights as the BIOS font writer.
    if !(1..=32).contains(&height) {
        return None;
    }
    let rows: u8 = match rows.selection {
        0 => rows.custom,
        1 => 14,
        3 => 43,
        _ => 25, // IBM also treats row selectors above three as 25 rows.
    };
    write_vector(memory, crate::interrupts::GRAPHICS_FONT, pointer);
    memory
        .write_u16(CHARACTER_HEIGHT, height)
        .expect("font height");
    memory
        .write_u8(LAST_ROW, rows.wrapping_sub(1))
        .expect("video rows");
    Some(())
}
