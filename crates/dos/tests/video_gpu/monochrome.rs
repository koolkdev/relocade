use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};

fn assert_monochrome(image: &[u8], height: usize) {
    for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (index % 720, index / 720);
        let gray = (x < 8 && y == height - 1) // Space with an eight-dot underline.
            || ((9..18).contains(&x) && y < height) // Full block extends into dot nine.
            || ((27..36).contains(&x) && y < height) // Reverse-video space.
            || (x >= 711 && (24 * height..25 * height).contains(&y));
        let white = (18..27).contains(&x) && y < height;
        assert_eq!(
            *pixel,
            if white {
                [255, 255, 255, 255]
            } else if gray {
                [170, 170, 170, 255]
            } else {
                [0, 0, 0, 255]
            },
            "pixel ({x}, {y}), font height {height}"
        );
    }
}

#[test]
fn guest_monochrome_text_renders_attributes_and_tracks_underline_through_font_changes() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        for (profile, lines, height, restore_font) in [(0, 350, 14, 0x11), (2, 400, 16, 0x14)] {
            let capture = run(
                &mut dos,
                &[
                    0xb8, profile, 0x12, 0xbb, 0x30, 0, 0xcd, 0x10, // Scanline preference.
                    0xb8, 7, 0, 0xcd, 0x10, // BIOS mode 07h.
                    0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
                    0xb8, 0, 0xb0, 0x8e, 0xc0, // ES = B000.
                    0x26, 0xc7, 0x06, 0, 0, b' ', 1, // Underlined space.
                    0x26, 0xc7, 0x06, 2, 0, 0xdb, 7, // Normal full block.
                    0x26, 0xc7, 0x06, 4, 0, 0xdb, 15, // Intense full block.
                    0x26, 0xc7, 0x06, 6, 0, b' ', 0x70, // Reverse-video space.
                    0x26, 0xc7, 0x06, 0x9e, 0x0f, 0xdb, 7, // Last visible cell.
                    0xc3,
                ],
            )
            .await;
            let image = gpu.render(&capture, [720, lines]).await;
            assert_monochrome(&image, height);
            for (font, height) in [(0x12, 8), (restore_font, height)] {
                let changed = run(
                    &mut dos,
                    &[
                        0xb8, font, 0x11, 0x31, 0xdb, 0xcd,
                        0x10, // Load font and change height.
                        0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide the reshaped cursor.
                        0xc3,
                    ],
                )
                .await;
                assert_monochrome(&gpu.render(&changed, [720, lines]).await, height);
            }
            // The color preset restores both the aperture and the DAC palette.
            let mut program = vec![0xb8, 0, 0x12, 0xbb, 0x30, 0, 0xcd, 0x10];
            program.extend_from_slice(TEXT_WITH_RED_BLOCK);
            let color = run(&mut dos, &program).await;
            assert_eq!(
                &gpu.render(&color, [640, 200]).await[..4],
                &[170, 0, 0, 255]
            );
            assert_eq!(gpu.render(&capture, [720, lines]).await, image);
        }
    });
}
