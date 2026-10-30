use super::*;

pub(super) fn append(cases: &mut Vec<Case>) {
    let mut vga = text_device();
    geometry(&mut vga, 1, 1, 1);
    for (address, color) in [
        (0xd001, 1),
        (0xa003, 2),
        (0xa002, 4),
        (0x4005, 6),
        (0x9001, 14),
    ] {
        physical_cell(&mut vga, address, 0xdb, color);
    }
    indexed(&mut vga, 0x3d4, 12, 0xd0);
    indexed(&mut vga, 0x3d4, 13, 1);
    for (name, mode, underline, color) in [
        ("byte addressing at D001", 0xe3, 0, [0, 0, 170, 255]),
        (
            "word wrap from counter bit fifteen",
            0xa3,
            0,
            [0, 170, 0, 255],
        ),
        (
            "word wrap from counter bit thirteen",
            0x83,
            0,
            [170, 0, 0, 255],
        ),
        ("doubleword rotation", 0xe3, 0x40, [170, 85, 0, 255]),
        (
            "row scan replaces address bit fourteen",
            0xe1,
            0,
            [255, 255, 85, 255],
        ),
    ] {
        indexed(&mut vga, 0x3d4, 0x17, mode);
        indexed(&mut vga, 0x3d4, 0x14, underline);
        cases.push(Case {
            name,
            display: vga.snapshot(),
            size: [8, 1],
            pixels: vec![(0, 0, color)],
        });
    }

    let mut vga = text_device();
    font(&mut vga, 0, b'A', &[0, 0x10]);
    font(&mut vga, 0, b'B', &[0x08]);
    font(&mut vga, 0, b'C', &[0x80]);
    for (address, character) in [(0x246a, b'A'), (0x24be, b'B'), (2, b'C')] {
        physical_cell(&mut vga, address, character, 7);
    }
    geometry(&mut vga, 40, 8, 4);
    for (index, value) in [(0x13, 21), (12, 0x12), (13, 0x34), (8, 0x21), (0x18, 3)] {
        indexed(&mut vga, 0x3d4, index, value);
    }
    indexed(&mut vga, 0x3c4, 1, 0);
    attribute(&mut vga, 0x13, 2);
    attribute(&mut vga, 0x10, 0x2c);
    cases.push(Case {
        name: "byte panning and preset rows survive a stride and split",
        display: vga.snapshot(),
        size: [360, 8],
        pixels: vec![
            (0, 0, GRAY),
            (1, 0, BLACK),
            (0, 3, BLACK),
            (1, 3, GRAY),
            (0, 4, GRAY),
            (3, 4, BLACK),
        ],
    });

    let mut vga = text_device();
    geometry(&mut vga, 4, 1, 1);
    set_cell(&mut vga, 0, 0xdb, 4);
    set_cell(&mut vga, 1, 0xdb, 1);
    indexed(&mut vga, 0x3d4, 0x17, 0xab); // Count by two.
    cases.push(Case {
        name: "count by two repeats a character",
        display: vga.snapshot(),
        size: [32, 1],
        pixels: vec![
            (0, 0, [170, 0, 0, 255]),
            (8, 0, [170, 0, 0, 255]),
            (16, 0, [0, 0, 170, 255]),
        ],
    });
    indexed(&mut vga, 0x3d4, 0x14, 0x20); // Count by four overrides count by two.
    cases.push(Case {
        name: "count by four overrides count by two",
        display: vga.snapshot(),
        size: [32, 1],
        pixels: vec![(16, 0, [170, 0, 0, 255]), (24, 0, [170, 0, 0, 255])],
    });
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
}
