// Literal programmable-text cases shared by CPU-reference and GPU checks.
use crate::common::fixture::{set_cell, text_device};
use relocade_vga::DisplaySnapshot;

pub struct Case {
    pub name: &'static str,
    pub display: DisplaySnapshot,
    pub size: [u32; 2],
    pub pixels: Vec<(u32, u32, [u8; 4])>,
}
const BLACK: [u8; 4] = [0, 0, 0, 255];
const GRAY: [u8; 4] = [170, 170, 170, 255];

fn indexed(io: &relocade_machine::IoBus, port: u16, index: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}
fn attribute(io: &relocade_machine::IoBus, index: u8, value: u8) {
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, u32::from(index));
    io.write_port(0x3c0, 1, u32::from(value));
    io.write_port(0x3c0, 1, 0x20);
}
fn palette(io: &relocade_machine::IoBus, index: u8, color: [u8; 3]) {
    io.write_port(0x3c8, 1, u32::from(index));
    for component in color {
        io.write_port(0x3c9, 1, u32::from(component));
    }
}
fn geometry(io: &relocade_machine::IoBus, columns: u8, lines: u8, glyph_height: u8) {
    indexed(io, 0x3d4, 0x11, 0);
    indexed(io, 0x3d4, 1, columns - 1);
    indexed(io, 0x3d4, 7, 0);
    indexed(io, 0x3d4, 9, glyph_height - 1);
    indexed(io, 0x3d4, 0x12, lines - 1);
}
fn font(io: &relocade_machine::IoBus, bank: u32, character: u8, rows: &[u8]) {
    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        io.write_port(port, 2, value);
    }
    for (row, value) in rows.iter().enumerate() {
        io.write_mmio(
            0xa0000 + bank + u32::from(character) * 32 + row as u32,
            1,
            u64::from(*value),
        );
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

fn physical_cell(io: &relocade_machine::IoBus, address: u32, character: u8, cell_attribute: u8) {
    indexed(io, 0x3c4, 4, 6);
    indexed(io, 0x3ce, 5, 0);
    indexed(io, 0x3ce, 6, 5);
    for (mask, value) in [(1, character), (2, cell_attribute)] {
        indexed(io, 0x3c4, 2, mask);
        io.write_mmio(0xa0000 + address, 1, u64::from(value));
    }
    indexed(io, 0x3c4, 2, 3);
    indexed(io, 0x3c4, 4, 2);
    indexed(io, 0x3ce, 5, 0x10);
    indexed(io, 0x3ce, 6, 0x0e);
}

#[path = "programmed_text/addressing.rs"]
mod addressing;
#[path = "programmed_text/colors.rs"]
mod colors;
#[path = "programmed_text/fonts.rs"]
mod fonts;

pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    fonts::append(&mut cases);
    colors::append(&mut cases);
    addressing::append(&mut cases);
    cases
}
