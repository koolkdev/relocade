//! Firmware for the banked DISPI adapter.
pub(super) mod vbe;

use crate::video::{
    COLOR_TEXT_80X25,
    modes::{self, ModeSet},
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::dispi;

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    vbe::initialize(memory);
    // Keep the BIOS text console available until the guest selects a VBE mode.
    assert!(
        modes::select_scanlines(memory, 2),
        "initial scanline profile"
    );
    set_mode(memory, io, COLOR_TEXT_80X25).expect("initial DISPI text mode");
}

/// Leave packed access before common VGA font loading and mode programming.
pub fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
    let mode = ModeSet::prepare(memory, request)?;
    io.write_port(dispi::INDEX_PORT, 2, u32::from(dispi::ENABLE));
    io.write_port(dispi::DATA_PORT, 2, u32::from(dispi::DISABLED));
    Some(mode.apply(memory, io))
}
