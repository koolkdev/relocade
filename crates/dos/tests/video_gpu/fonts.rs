use super::{common, run, Gpu};

#[test]
fn bios_custom_fonts_and_dual_banks_render_fifty_rows_and_preserve_old_captures() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let mut program = vec![
            0xb8, 2, 0x12, 0xbb, 0x30, 0, 0xcd, 0x10, // Select the 400-line profile.
            0xb8, 3, 0, 0xcd, 0x10, // Set mode 03h with 16-high font.
            0xb8, 0x12, 0x11, 0x31, 0xdb, 0xcd, 0x10, // Load 8-high font and use 50 rows.
            0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor after the height change.
        ];
        program.extend_from_slice(LOAD_BANK_FOUR);
        program.extend_from_slice(&[
            0xb8, 3, 0x11, 0xbb, 0x20, 0, 0xcd, 0x10, // Font A=bank 4, font B=bank 0.
            0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
            0x26, 0xc7, 0x06, 0, 0, b'A', 15, // Custom glyph; bit 3 selects font A.
            0x26, 0xc7, 0x06, 2, 0, b'A', 4, // Built-in glyph from font B, red.
            0x26, 0xc7, 0x06, 0x3e, 0x1f, b'A', 15, // Row 49, column 79.
            0xc3,
        ]);
        program.resize(0x100, 0x90);
        program.extend_from_slice(&[0x80, 0x40, 0x20, 0x10, 8, 4, 2, 1]);
        let diagonal = run(&mut dos, &program).await;
        let pixels = gpu.render(&diagonal, [640, 400]).await;
        assert_custom_font(&pixels, false);

        let mut program = LOAD_BANK_FOUR.to_vec();
        program.push(0xc3);
        program.resize(0x100, 0x90);
        program.extend_from_slice(&[255; 8]);
        let solid = run(&mut dos, &program).await;
        assert!(std::ptr::eq(
            diagonal.display.plane(0),
            solid.display.plane(0)
        ));
        assert!(std::ptr::eq(
            diagonal.display.plane(1),
            solid.display.plane(1)
        ));
        assert_custom_font(&gpu.render(&solid, [640, 400]).await, true);
        drop(dos);
        assert_eq!(gpu.render(&diagonal, [640, 400]).await, pixels);
    });
}

const LOAD_BANK_FOUR: &[u8] = &[
    0x0e, 0x07, 0xbd, 0, 2, // ES:BP = CS:0200, after the guest code.
    0xb8, 0, 0x11, 0xbb, 4, 8, // AX=1100h, eight bytes per glyph, bank 4.
    0xb9, 1, 0, 0xba, 65, 0, 0xcd, 0x10, // Load character A only.
];

fn assert_custom_font(image: &[u8], solid: bool) {
    let builtin = [0x7c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0];
    for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (index % 640, index / 640);
        let custom = (x < 8 && y < 8 && (solid || x == y))
            || (x >= 632 && y >= 392 && (solid || x - 632 == y - 392));
        let red = (8..16).contains(&x) && y < 8 && builtin[y] & (0x80 >> (x - 8)) != 0;
        let expected = if custom {
            [170, 170, 170, 255] // Dual fonts consume attribute bit 3 instead of intensity.
        } else if red {
            [170, 0, 0, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "pixel ({x}, {y})");
    }
}

#[test]
fn bios_color_text_profiles_render_both_widths() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        for (selection, lines, glyph) in [
            (0, 200, &[0x7c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0][..]),
            (
                1,
                350,
                &[
                    0, 0, 0x38, 0x6c, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0, 0, 0,
                ][..],
            ),
            (
                2,
                400,
                &[
                    0, 0, 0x38, 0x6c, 0xc6, 0xc6, 0xc6, 0xfe, 0xc6, 0xc6, 0xc6, 0xc6, 0, 0, 0, 0,
                ][..],
            ),
        ] {
            for (mode, width, last_cell) in [(1, 320, 0x07ceu16), (3, 640, 0x0f9e)] {
                let [low, high] = last_cell.to_le_bytes();
                let capture = run(
                    &mut dos,
                    &[
                        0xb8, selection, 0x12, 0xbb, 0x30, 0, 0xcd,
                        0x10, // Scanline preference.
                        0xb8, mode, 0, 0xcd, 0x10, // Set text mode with the profile's font.
                        0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
                        0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
                        0x26, 0xc7, 0x06, 0, 0, b'A', 4, // Native built-in glyph, red.
                        0x26, 0xc7, 0x06, low, high, 0xdb,
                        4, // Last cell of the 25-row screen.
                        0xc3,
                    ],
                )
                .await;
                let image = gpu.render(&capture, [width, lines]).await;
                for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
                    let (x, y) = (index % width as usize, index / width as usize);
                    let red = (x < 8 && y < glyph.len() && glyph[y] & (0x80 >> x) != 0)
                        || (x >= width as usize - 8 && y >= lines as usize - glyph.len());
                    assert_eq!(
                        *pixel,
                        if red {
                            [170, 0, 0, 255]
                        } else {
                            [0, 0, 0, 255]
                        },
                        "mode {mode}, profile {selection}, pixel ({x}, {y})"
                    );
                }
            }
        }
    });
}
