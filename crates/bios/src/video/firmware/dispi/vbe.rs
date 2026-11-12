//! Banked VESA BIOS services for indexed and direct-color modes.
mod information;
mod layout;
mod modes;
mod palette;
mod state;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext,
};
use relocade_vga::DispiAdapter;
use std::cell::RefCell;

pub(super) use information::initialize;
use modes::Mode;

#[interrupt(context(call), args(function = al))]
pub(in crate::video::firmware) async fn dispatch(
    call: &mut CpuContext<'_>,
    card: &RefCell<DispiAdapter>,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => information::controller(card),
            0x01 => information::mode(card),
            0x02 => modes::set(card),
            0x03 => modes::current(card),
            0x04 => state::dispatch(card),
            0x05 => modes::window(card),
            0x06 => layout::scanline(card),
            0x07 => layout::display_start(card),
            0x08 => palette::format(card),
            _ => unsupported,
        }
    );
}

// VBE reports status in AX and preserves flags, including carry.
#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}

fn available(card: &RefCell<DispiAdapter>) -> bool {
    card.borrow().ports_enabled()
}
