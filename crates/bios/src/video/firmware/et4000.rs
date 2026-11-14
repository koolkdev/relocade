//! Firmware for the 1-MiB ET4000AX board, initially in six-bit indexed DAC mode.
mod modes;

use crate::video::{modes as legacy, COLOR_TEXT_80X25};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::Et4000Ax;
use std::cell::RefCell;

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus, card: &RefCell<Et4000Ax>) {
    assert!(
        legacy::select_scanlines(memory, 2),
        "initial scanline profile"
    );
    set_mode(memory, io, card, COLOR_TEXT_80X25).expect("initial ET4000 text mode");
}

pub fn set_mode(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<Et4000Ax>,
    request: u8,
) -> Option<u8> {
    if !card.borrow().ports_enabled() {
        return None;
    }
    if let Some(mode) = modes::Mode::from_bios_number(request & 0x7f) {
        mode.activate(memory, io, card, request & 0x80 != 0);
        return Some(0x20);
    }
    // Validate the request and guest parameter table before changing the card.
    let mode = legacy::ModeSet::prepare(memory, request)?;
    io.with_context(|context| card.borrow_mut().prepare_legacy_mode(context));
    Some(mode.apply(memory, io))
}
