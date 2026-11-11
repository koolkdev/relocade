use super::*;

pub(super) fn append(cases: &mut Vec<Case>) {
    let vga = text_device();
    geometry(&vga.io, 1, 1, 1);
    set_cell(&vga.io, 0, 0xdb, 7);
    attribute(&vga.io, 7, 0x3f);
    attribute(&vga.io, 0x14, 0x0c);
    palette(&vga.io, 255, [63, 17, 1]);
    cases.push(Case {
        name: "last DAC entry spans the final packed word",
        display: vga.snapshot(),
        size: [8, 1],
        pixels: vec![(0, 0, [255, 69, 4, 255])],
    });
    palette(&vga.io, 255, [1, 63, 17]);
    cases.push(Case {
        name: "palette-only update changes captured pixels",
        display: vga.snapshot(),
        size: [8, 1],
        pixels: vec![(0, 0, [4, 255, 69, 255])],
    });

    // The raw address contains both a register index and the display-enable bit.
    for (address, name, pixel) in [
        (0x14, "nonzero attribute index with display disabled", BLACK),
        (
            0x34,
            "same attribute index with display enabled",
            [4, 255, 69, 255],
        ),
    ] {
        vga.io.read_port(0x3da, 1);
        vga.io.write_port(0x3c0, 1, address);
        cases.push(Case {
            name,
            display: vga.snapshot(),
            size: [8, 1],
            pixels: vec![(0, 0, pixel)],
        });
    }

    attribute(&vga.io, 0x12, 0); // Attribute plane enable selects entry zero.
    attribute(&vga.io, 0, 0);
    palette(&vga.io, 0xc0, [0, 0, 0]);
    cases.push(Case {
        name: "attribute plane enable masks text colors",
        display: vga.snapshot(),
        size: [8, 1],
        pixels: vec![(0, 0, BLACK)],
    });
    let vga = text_device();
    geometry(&vga.io, 3, 2, 2);
    indexed(&vga.io, 0x3d4, 0x14, 1);
    set_cell(&vga.io, 0, b' ', 1);
    set_cell(&vga.io, 1, b' ', 0x91);
    set_cell(&vga.io, 2, 0xdb, 1);
    attribute(&vga.io, 0x10, 0x84); // Color-select P4/P5; background intensity enabled.
    attribute(&vga.io, 0x14, 0x0a);
    vga.io.write_port(0x3c6, 1, 0x7f);
    palette(&vga.io, 0x20, [0, 0, 0]);
    palette(&vga.io, 0x21, [1, 17, 63]);
    palette(&vga.io, 0x29, [63, 0, 0]);
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

    let vga = text_device();
    geometry(&vga.io, 2, 2, 2);
    indexed(&vga.io, 0x3c4, 1, 0);
    indexed(&vga.io, 0x3d4, 0x14, 1);
    attribute(&vga.io, 0x13, 8);
    set_cell(&vga.io, 0, b' ', 1);
    set_cell(&vga.io, 1, 0xc4, 1);
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
}
