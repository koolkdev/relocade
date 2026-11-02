//! INT 10h string formats and cursor-update policy.
use crate::screen::Output;
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};

pub(super) struct StringRun {
    page: u8,
    /// Starting row in the high byte and column in the low byte.
    position: u16,
    source: FarPointer,
    count: u16,
}

/// Writes characters with one attribute, preserving the page's stored cursor.
#[interrupt(context(memory, io),
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }, attribute = bl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(super) fn write_text(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: StringRun,
    attribute: u8,
) -> Option<()> {
    write(memory, io, run, Some(attribute), false)
}

/// Writes characters with one attribute and moves the page's stored cursor.
#[interrupt(context(memory, io),
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }, attribute = bl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(super) fn write_text_and_move_cursor(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: StringRun,
    attribute: u8,
) -> Option<()> {
    write(memory, io, run, Some(attribute), true)
}

/// Writes character/attribute pairs, preserving the page's stored cursor.
#[interrupt(context(memory, io),
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(super) fn write_cells(memory: &mut MemoryView<'_>, io: &IoBus, run: StringRun) -> Option<()> {
    write(memory, io, run, None, false)
}

/// Writes character/attribute pairs and moves the page's stored cursor.
#[interrupt(context(memory, io),
    args(run = StringRun { page: bh, position: dx, source: ptr(es, bp), count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(super) fn write_cells_and_move_cursor(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: StringRun,
) -> Option<()> {
    write(memory, io, run, None, true)
}

fn write(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: StringRun,
    attribute: Option<u8>,
    move_cursor: bool,
) -> Option<()> {
    if run.count == 0 {
        return Some(());
    }
    let mut output = Output::new(memory, io, run.page)?;
    output.set_position(run.position);
    output.write_string(run.source, run.count, attribute);
    if move_cursor {
        output.commit_cursor();
    }
    Some(())
}
