//! Connected devices and guest-visible programming shared by VGA fixtures.
#![allow(dead_code)]
use relocade_machine::{Clock, IoBus};
use relocade_vga::{registers::text_mode, Vga};
use std::{cell::RefCell, rc::Rc};

pub struct ConnectedVga {
    pub device: Rc<RefCell<Vga>>,
    pub io: IoBus,
}

impl ConnectedVga {
    pub fn new(clock: Clock) -> Self {
        let device = Rc::new(RefCell::new(Vga::with_clock(clock)));
        let io = IoBus::new(device.clone(), |io| {
            io.map_mmio([relocade_vga::MEMORY_RANGE]);
            io.map_ports(relocade_vga::PORT_RANGES);
        });
        Self { device, io }
    }

    // Return an owned observation so callers can await rendering without
    // retaining a RefCell borrow of the live device.
    pub fn snapshot(&self) -> relocade_vga::DisplaySnapshot {
        self.device.borrow().snapshot()
    }
}

pub fn initialize_font(io: &IoBus) {
    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        io.write_port(port, 2, value);
    }
    for (character, glyph) in text_mode::FONT.as_chunks::<8>().0.iter().enumerate() {
        for (row, byte) in glyph.iter().enumerate() {
            io.write_mmio(
                0xa0000 + character as u32 * 32 + row as u32,
                1,
                u64::from(*byte),
            );
        }
    }
    for (port, value) in [
        (0x3c4, 0x0302),
        (0x3c4, 0x0204),
        (0x3ce, 0x1005),
        (0x3ce, 0x0e06),
    ] {
        io.write_port(port, 2, value);
    }
}

pub fn text_device() -> ConnectedVga {
    let vga = ConnectedVga::new(relocade_clock_test_support::ManualClock::new().clock());
    initialize_font(&vga.io);
    for cell in 0..2000 {
        set_cell(&vga.io, cell, b' ', 7);
    }
    vga.io.write_port(0x3d4, 2, 0x200a);
    vga
}

pub fn set_cell(io: &IoBus, cell: usize, character: u8, attribute: u8) {
    io.write_mmio(
        0xb8000 + cell as u32 * 2,
        2,
        u64::from(u16::from_le_bytes([character, attribute])),
    );
}
