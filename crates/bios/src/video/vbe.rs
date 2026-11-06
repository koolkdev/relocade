//! Banked VESA BIOS services. The 1.2 data layouts precede full 1.2 compliance:
//! state, logical scanline, display-start and DAC-format services remain unsupported.
mod information;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, IoBus,
};
use relocade_vga::registers::dispi;

pub(super) use information::initialize;

/// BIOS mode numbers and geometry belong here; DISPI owns hardware validation.
struct Mode {
    number: u16,
    width: u16,
    height: u16,
    bits_per_pixel: u8,
}

impl Mode {
    const INDEXED_640_480: Self = Self {
        number: 0x101,
        width: 640,
        height: 480,
        bits_per_pixel: 8,
    };

    fn from_number(number: u16) -> Option<Self> {
        (number == Self::INDEXED_640_480.number).then_some(Self::INDEXED_640_480)
    }
}

#[interrupt(context(call), args(function = al))]
pub(super) async fn dispatch(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => information::controller,
            0x01 => information::mode,
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
