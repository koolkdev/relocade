//! Banked VESA BIOS services for indexed and direct-color modes.
mod information;
mod layout;
mod modes;
mod palette;
mod state;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, IoBus,
};
use relocade_vga::{registers::dispi, Vga};
use std::cell::RefCell;

pub(super) use information::initialize;
use modes::Mode;

#[interrupt(context(call), args(function = al))]
pub(super) async fn dispatch(call: &mut CpuContext<'_>, vga: &RefCell<Vga>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => information::controller,
            0x01 => information::mode,
            0x02 => modes::set,
            0x03 => modes::current,
            0x04 => state::dispatch(vga),
            0x05 => modes::window,
            0x06 => layout::scanline,
            0x07 => layout::display_start(vga),
            0x08 => palette::format,
            _ => unsupported,
        }
    );
}

// VBE reports status in AX and preserves flags, including carry.
#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}

fn read_register(io: &IoBus, register: u16) -> u16 {
    let index = io.read_port(dispi::INDEX_PORT, 2);
    io.write_port(dispi::INDEX_PORT, 2, u32::from(register));
    let value = io.read_port(dispi::DATA_PORT, 2) as u16;
    io.write_port(dispi::INDEX_PORT, 2, index);
    value
}

fn available(io: &IoBus) -> bool {
    read_register(io, dispi::ID) == dispi::INTERFACE_ID
}

fn write_register(io: &IoBus, register: u16, value: u16) {
    io.write_port(dispi::INDEX_PORT, 2, u32::from(register));
    io.write_port(dispi::DATA_PORT, 2, u32::from(value));
}
