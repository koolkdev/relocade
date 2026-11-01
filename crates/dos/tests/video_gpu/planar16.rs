use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};

const COLORS: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [0, 0, 170, 255],
    [0, 170, 0, 255],
    [0, 170, 170, 255],
    [170, 0, 0, 255],
    [170, 0, 170, 255],
    [170, 85, 0, 255],
    [170, 170, 170, 255],
    [85, 85, 85, 255],
    [85, 85, 255, 255],
    [85, 255, 85, 255],
    [85, 255, 255, 255],
    [255, 85, 85, 255],
    [255, 85, 255, 255],
    [255, 255, 85, 255],
    [255, 255, 255, 255],
];

fn assert_image(image: &[u8], brown: [u8; 4]) {
    let pixels = image.as_chunks::<4>().0;
    for (offset, pixel) in pixels.iter().enumerate() {
        let expected = if offset == 6 || offset == pixels.len() - 1 {
            brown
        } else if offset < 16 {
            COLORS[offset]
        } else {
            COLORS[1]
        };
        assert_eq!(*pixel, expected, "pixel {offset}");
    }
}

#[test]
fn guest_planar_modes_masked_pixels_palette_and_text_return() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        for (mode, size, brown_index) in [
            (0x0d, [320, 200], 6),
            (0x0e, [640, 200], 6),
            (0x10, [640, 350], 0x14),
            (0x12, [640, 480], 0x14),
        ] {
            let mut dos = common::dos().await;
            let bytes = (size[0] * size[1] / 8) as u16;
            let [low, high] = bytes.to_le_bytes();
            let mut program = vec![
                0xb8, mode, 0, 0xcd, 0x10, // Set the BIOS mode.
                0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                0xba, 0xce, 3, 0xb8, 5, 2, 0xef, // CPU write mode 2.
                0x31, 0xff, 0xb9, low, high, // DI = 0, CX = plane bytes.
                0xb0, 1, 0xfc, 0xf3, 0xaa, // Fill all visible pixels blue.
            ];
            // Set the first sixteen pixels individually through the CPU latches.
            for pixel in 0..16u8 {
                program.extend_from_slice(&[0xb8, 8, 0x80 >> (pixel & 7), 0xef]); // Bit mask.
                program.extend_from_slice(&[0x26, 0xa0, pixel / 8, 0]); // Read latches.
                program.extend_from_slice(&[0x26, 0xc6, 0x06, pixel / 8, 0, pixel]);
            }
            let [low, high] = (bytes - 1).to_le_bytes();
            program.extend_from_slice(&[
                0xb8, 8, 1, 0xef, // Rightmost bit in the last plane byte.
                0x26, 0xa0, low, high, 0x26, 0xc6, 0x06, low, high, 6, // Last pixel is brown.
                0xc3,
            ]);
            let graphics = run(&mut dos, &program).await;
            let original = gpu.render(&graphics, size).await;
            assert_image(&original, COLORS[6]);

            // The 200-line and higher-resolution presets use different DAC indices.
            let mut recolor = vec![0xba, 0xc8, 3, 0xb0, brown_index, 0xee]; // DAC index.
            recolor.extend_from_slice(&[
                0x42, 0xb0, 63, 0xee, // Red = 63.
                0x30, 0xc0, 0xee, 0xee, // Green/blue = 0.
                0xc3,
            ]);
            let changed = run(&mut dos, &recolor).await;
            for plane in 0..4 {
                assert!(std::ptr::eq(
                    graphics.display.plane(plane),
                    changed.display.plane(plane)
                ));
            }
            assert_image(&gpu.render(&changed, size).await, [255, 0, 0, 255]);
            let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
            drop(dos);
            let restored = gpu.render(&text, [640, 200]).await;
            assert_eq!(&restored[..4], &[170, 0, 0, 255]);
            assert_eq!(&restored[8 * 4..9 * 4], &[0, 0, 0, 255]);
            assert_eq!(gpu.render(&graphics, size).await, original);
        }
    });
}
