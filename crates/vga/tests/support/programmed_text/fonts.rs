use super::*;

pub(super) fn append(cases: &mut Vec<Case>) {
    let mut vga = text_device();
    let mut rows = [0; 16];
    rows[15] = 0x80;
    font(&mut vga, 0xe000, b'A', &rows);
    rows[15] = 0x40;
    font(&mut vga, 0, b'A', &rows);
    rows[15] = 0x20;
    font(&mut vga, 0x2000, b'A', &rows);
    set_cell(&mut vga, 0, b'A', 7);
    set_cell(&mut vga, 1, b'A', 15);
    geometry(&mut vga, 2, 16, 16);
    indexed(&mut vga, 0x3c4, 3, 0x13); // A at zero, B at E000.
    cases.push(Case {
        name: "extended font banks select A with attribute bit three",
        display: vga.snapshot(),
        size: [16, 16],
        pixels: vec![(0, 15, GRAY), (1, 15, BLACK), (8, 15, BLACK), (9, 15, GRAY)],
    });
    indexed(&mut vga, 0x3c4, 4, 0); // Alias B to 2000 and restore intensity.
    cases.push(Case {
        name: "nonextended font alias disables dual-font selection",
        display: vga.snapshot(),
        size: [16, 16],
        pixels: vec![
            (0, 15, BLACK),
            (2, 15, GRAY),
            (9, 15, BLACK),
            (10, 15, [255, 255, 255, 255]),
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
    set_cell(&mut vga, 0, b'A', 7);
    geometry(&mut vga, 1, 3, 2);
    indexed(&mut vga, 0x3d4, 9, 0x81);
    cases.push(Case {
        name: "partial doubled row retains physical text scanlines",
        display: vga.snapshot(),
        size: [8, 3],
        pixels: vec![
            (0, 0, GRAY),
            (1, 0, BLACK),
            (0, 1, GRAY),
            (1, 1, BLACK),
            (0, 2, BLACK),
            (1, 2, GRAY),
        ],
    });
}
