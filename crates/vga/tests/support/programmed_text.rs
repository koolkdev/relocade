// Literal programmable-text cases shared by CPU-reference and GPU checks.
use crate::common::fixture::{set_cell, text_device};
use relocade_machine::Device;
use relocade_vga::DisplaySnapshot;

pub struct Case {
    pub name: &'static str,
    pub display: DisplaySnapshot,
    pub size: [u32; 2],
    pub pixels: Vec<(u32, u32, [u8; 4])>,
}
const BLACK: [u8; 4] = [0, 0, 0, 255];
const GRAY: [u8; 4] = [170, 170, 170, 255];

fn indexed(vga: &mut impl Device, port: u16, index: u8, value: u8) {
    vga.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}
fn attribute(vga: &mut impl Device, index: u8, value: u8) {
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, u32::from(index));
    vga.write_port(0x3c0, 1, u32::from(value));
    vga.write_port(0x3c0, 1, 0x20);
}
fn palette(vga: &mut impl Device, index: u8, color: [u8; 3]) {
    vga.write_port(0x3c8, 1, u32::from(index));
    for component in color {
        vga.write_port(0x3c9, 1, u32::from(component));
    }
}
fn geometry(vga: &mut impl Device, columns: u8, lines: u8, glyph_height: u8) {
    indexed(vga, 0x3d4, 0x11, 0);
    indexed(vga, 0x3d4, 1, columns - 1);
    indexed(vga, 0x3d4, 7, 0);
    indexed(vga, 0x3d4, 9, glyph_height - 1);
    indexed(vga, 0x3d4, 0x12, lines - 1);
}
fn font(vga: &mut impl Device, bank: u32, character: u8, rows: &[u8]) {
    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
    for (row, value) in rows.iter().enumerate() {
        vga.write_mmio(
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
        vga.write_port(port, 2, value);
    }
}

fn physical_cell(vga: &mut impl Device, address: u32, character: u8, cell_attribute: u8) {
    indexed(vga, 0x3c4, 4, 6);
    indexed(vga, 0x3ce, 5, 0);
    indexed(vga, 0x3ce, 6, 5);
    for (mask, value) in [(1, character), (2, cell_attribute)] {
        indexed(vga, 0x3c4, 2, mask);
        vga.write_mmio(0xa0000 + address, 1, u64::from(value));
    }
    indexed(vga, 0x3c4, 2, 3);
    indexed(vga, 0x3c4, 4, 2);
    indexed(vga, 0x3ce, 5, 0x10);
    indexed(vga, 0x3ce, 6, 0x0e);
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
