use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};

fn assert_two_color(image: &[u8], foreground: [u8; 4]) {
    for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (index % 640, index / 640);
        let lit = (y == 0 && matches!(x, 0 | 2 | 5 | 7))
            || ((8..16).contains(&x) && (16..32).contains(&y))
            || (matches!(x, 638 | 639) && y == 479);
        assert_eq!(
            *pixel,
            if lit { foreground } else { [0, 0, 0, 255] },
            "pixel ({x}, {y})"
        );
    }
}

#[test]
fn mode_11h_direct_writes_bios_drawing_palette_and_text_return() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let original = run(
            &mut dos,
            &[
                0xb8, 0x11, 0, 0xcd, 0x10, // Mode 11h.
                0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000; writes reach all planes.
                0x26, 0xc6, 0x06, 0, 0, 0xa5, // Eight pixels at the start of VRAM.
                0xb8, 1, 0x0c, 0x31, 0xdb, // AH=0C, color 1, page zero.
                0xb9, 0x7f, 2, 0xba, 0xdf, 1, 0xcd, 0x10, // Pixel (639, 479).
                0x49, 0xb0, 4, 0xcd, 0x10, // Pixel (638, 479), plane two only.
                0xb8, 0, 2, 0xba, 1, 1, 0xcd, 0x10, // Cursor at cell (1, 1).
                0xb8, 0xdb, 9, 0xb9, 1, 0, 0xcd, 0x10, // Full block; BL=0 still draws white.
                0xc3,
            ],
        )
        .await;
        let white = gpu.render(&original, [640, 480]).await;
        assert_two_color(&white, [255, 255, 255, 255]);
        let preserved = run(&mut dos, &[0xb8, 0x91, 0, 0xcd, 0x10, 0xc3]).await;
        for plane in 0..4 {
            assert_eq!(
                preserved.display.plane(plane),
                original.display.plane(plane)
            );
        }
        assert_two_color(
            &gpu.render(&preserved, [640, 480]).await,
            [255, 255, 255, 255],
        );
        let recolored = run(
            &mut dos,
            &[
                0xba, 0xc8, 3, 0xb0, 63, 0xee, 0x42, // DAC entry 63.
                0x30, 0xc0, 0xee, 0xb0, 63, 0xee, 0x30, 0xc0, 0xee, 0xc3,
            ],
        )
        .await;
        assert_two_color(&gpu.render(&recolored, [640, 480]).await, [0, 255, 0, 255]);
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        drop(dos);
        assert_eq!(&gpu.render(&text, [640, 200]).await[..4], &[170, 0, 0, 255]);
        assert_eq!(gpu.render(&original, [640, 480]).await, white);
    });
}
