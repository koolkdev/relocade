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

pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut vga = text_device();
    font(&mut vga, 0x4000, b'A', &[0x81, 0x42, 0x24, 0x18]);
    font(&mut vga, 0x4000, b'B', &[0x80, 0, 0, 0]);
    set_cell(&mut vga, 0, b'A', 7);
    set_cell(&mut vga, 42, b'B', 7);
    geometry(&mut vga, 40, 8, 4);
    indexed(&mut vga, 0x3d4, 0x13, 21); // Row padding is independent of visible width.
    indexed(&mut vga, 0x3c4, 1, 0); // Nine-dot characters.
    indexed(&mut vga, 0x3c4, 3, 1); // Map B at 4000.
    attribute(&mut vga, 0x13, 8); // No panning in nine-dot text.
    cases.push(Case {
        name: "geometry, stride and custom font",
        display: vga.snapshot(),
        size: [360, 8],
        pixels: vec![
            (0, 0, GRAY),
            (7, 0, GRAY),
            (8, 0, BLACK),
            (1, 1, GRAY),
            (0, 4, GRAY),
            (1, 4, BLACK),
        ],
    });

    let mut vga = text_device();
    font(&mut vga, 0, 0xc4, &[1]);
    font(&mut vga, 0x4000, 0xc4, &[0x80]);
    set_cell(&mut vga, 0, 0xc4, 7);
    set_cell(&mut vga, 1, 0xc4, 15);
    geometry(&mut vga, 2, 1, 1);
    indexed(&mut vga, 0x3c4, 1, 0);
    indexed(&mut vga, 0x3c4, 3, 4); // Map A at 4000, map B at zero.
    attribute(&mut vga, 0x13, 8);
    cases.push(Case {
        name: "dual font and ninth dot",
        display: vga.snapshot(),
        size: [18, 1],
        pixels: vec![
            (0, 0, BLACK),
            (7, 0, GRAY),
            (8, 0, GRAY),
            (9, 0, GRAY),
            (17, 0, BLACK),
        ],
    });
    attribute(&mut vga, 0x10, 8); // Disable line-graphics extension.
    cases.push(Case {
        name: "ninth dot disabled",
        display: vga.snapshot(),
        size: [18, 1],
        pixels: vec![(7, 0, GRAY), (8, 0, BLACK)],
    });

    let mut vga = text_device();
    geometry(&mut vga, 3, 2, 2);
    indexed(&mut vga, 0x3d4, 0x14, 1);
    set_cell(&mut vga, 0, b' ', 1);
    set_cell(&mut vga, 1, b' ', 0x91);
    set_cell(&mut vga, 2, 0xdb, 1);
    attribute(&mut vga, 0x10, 0x84); // Color-select P4/P5; background intensity enabled.
    attribute(&mut vga, 0x14, 0x0a);
    vga.write_port(0x3c6, 1, 0x7f);
    palette(&mut vga, 0x20, [0, 0, 0]);
    palette(&mut vga, 0x21, [1, 17, 63]);
    palette(&mut vga, 0x29, [63, 0, 0]);
    cases.push(Case {
        name: "palette, mask, underline and bright background",
        display: vga.snapshot(),
        size: [24, 2],
        pixels: vec![
            (0, 0, BLACK),
            (0, 1, [4, 69, 255, 255]),
            (8, 0, [255, 0, 0, 255]),
            (16, 0, [4, 69, 255, 255]),
        ],
    });

    let mut vga = text_device();
    let mut rows = [0; 16];
    rows[0] = 1;
    rows[15] = 0x80;
    font(&mut vga, 0xe000, b'A', &rows);
    set_cell(&mut vga, 0, b'A', 7);
    geometry(&mut vga, 1, 16, 16);
    indexed(&mut vga, 0x3c4, 3, 0x13);
    cases.push(Case {
        name: "sixteen-row font in last bank",
        display: vga.snapshot(),
        size: [8, 16],
        pixels: vec![(7, 0, GRAY), (0, 7, BLACK), (0, 15, GRAY)],
    });

    let mut vga = text_device();
    font(&mut vga, 0, b'A', &[0x80, 0x40]);
    font(&mut vga, 0, b'C', &[0x20, 0]);
    set_cell(&mut vga, 0, b'A', 7);
    set_cell(&mut vga, 4, b'C', 7);
    geometry(&mut vga, 2, 4, 2);
    indexed(&mut vga, 0x3d4, 0x13, 2);
    indexed(&mut vga, 0x3d4, 8, 1);
    indexed(&mut vga, 0x3d4, 0x18, 1);
    attribute(&mut vga, 0x13, 1);
    attribute(&mut vga, 0x10, 0x2c);
    cases.push(Case {
        name: "preset row, panning and split reset",
        display: vga.snapshot(),
        size: [16, 4],
        pixels: vec![
            (0, 0, GRAY),
            (1, 1, GRAY),
            (0, 2, GRAY),
            (1, 3, GRAY),
            (0, 3, BLACK),
        ],
    });
    let mut vga = text_device();
    geometry(&mut vga, 2, 2, 2);
    indexed(&mut vga, 0x3c4, 1, 0);
    indexed(&mut vga, 0x3d4, 0x14, 1);
    attribute(&mut vga, 0x13, 8);
    set_cell(&mut vga, 0, b' ', 1);
    set_cell(&mut vga, 1, 0xc4, 1);
    cases.push(Case {
        name: "underline preserves ordinary ninth-dot spacing",
        display: vga.snapshot(),
        size: [18, 2],
        pixels: vec![
            (7, 1, [0, 0, 170, 255]),
            (8, 1, BLACK),
            (16, 1, [0, 0, 170, 255]),
            (17, 1, [0, 0, 170, 255]),
        ],
    });

    let mut vga = text_device();
    font(&mut vga, 0, b'A', &[0x80, 0x40]);
    font(&mut vga, 0, b'B', &[0x40, 0x80]);
    set_cell(&mut vga, 0, b'A', 7);
    set_cell(&mut vga, 1, b'B', 4);
    geometry(&mut vga, 1, 4, 2);
    indexed(&mut vga, 0x3d4, 9, 0x81);
    indexed(&mut vga, 0x3d4, 0x0d, 1);
    indexed(&mut vga, 0x3d4, 0x18, 0);
    cases.push(Case {
        name: "split between scan-doubled rows",
        display: vga.snapshot(),
        size: [8, 4],
        pixels: vec![
            (1, 0, [170, 0, 0, 255]),
            (0, 1, GRAY),
            (0, 2, GRAY),
            (1, 3, GRAY),
            (0, 3, BLACK),
        ],
    });
    cases
}
