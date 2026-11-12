//! Firmware for the banked DISPI adapter.
pub(super) mod vbe;

use crate::video::{
    modes::{self, ModeSet},
    COLOR_TEXT_80X25,
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::DispiAdapter;
use std::cell::RefCell;

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus, card: &RefCell<DispiAdapter>) {
    vbe::initialize(memory);
    // Keep the BIOS text console available until the guest selects a VBE mode.
    assert!(
        modes::select_scanlines(memory, 2),
        "initial scanline profile"
    );
    set_mode(memory, io, card, COLOR_TEXT_80X25).expect("initial DISPI text mode");
}

/// Leave packed access before common VGA font loading and mode programming.
pub fn set_mode(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<DispiAdapter>,
    request: u8,
) -> Option<u8> {
    let mode = ModeSet::prepare(memory, request)?;
    io.with_context(|context| card.borrow_mut().prepare_legacy_mode(context));
    Some(mode.apply(memory, io))
}
