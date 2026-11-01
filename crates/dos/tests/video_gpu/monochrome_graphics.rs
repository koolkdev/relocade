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

fn assert_monochrome_pattern(image: &[u8], blink_on: bool) {
    for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let level = match index {
            1 | 5 => 170,
            3 | 7 => 255,
            2 | 6 if blink_on => 255,
            _ => 0,
        };
        assert_eq!(*pixel, [level, level, level, 255], "pixel {index}");
    }
}

#[test]
fn mode_0fh_monochrome_blink_pages_and_retained_captures() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let first = run(
            &mut dos,
            &[
                0xb8, 0x0f, 0, 0xcd, 0x10, // Mode 0Fh.
                0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                0xba, 0xc4, 3, 0xb8, 2, 1, 0xef, // Plane zero: video.
                0x26, 0xc6, 0x06, 0, 0, 0x55, // Alternating video bits.
                0xb8, 2, 4, 0xef, // Plane two: intensity.
                0x26, 0xc6, 0x06, 0, 0, 0x33, // Alternate intensity every two pixels.
                0xb8, 2, 0x0a, 0xef, // Planes one and three must stay invisible.
                0x26, 0xc6, 0x06, 0, 0, 0xff, // Set their first eight bits.
                0xb8, 5, 0x0c, 0xbb, 0, 1, // Bright pixel on hidden page one.
                0xb9, 0x7f, 2, 0xba, 0x5d, 1, 0xcd, 0x10, // (639, 349).
                0xb8, 0, 2, 0xba, 1, 1, 0xcd, 0x10, // Page one's cursor at cell (1, 1).
                0xb8, 0xdb, 9, 0xb3, 1, 0xb9, 1, 0, 0xcd, 0x10, // Normal white block.
                0xc3,
            ],
        )
        .await;
        let original = gpu.render(&first, [640, 350]).await;
        assert_monochrome_pattern(&original, first.display.blink_on);
        // A real device phase change publishes the same memory with new colors.
        let opposite = loop {
            let capture = common::next_capture(&dos).await;
            if capture.display.blink_on != first.display.blink_on {
                break capture;
            }
        };
        for plane in 0..4 {
            assert!(std::ptr::eq(
                first.display.plane(plane),
                opposite.display.plane(plane)
            ));
        }
        assert_monochrome_pattern(
            &gpu.render(&opposite, [640, 350]).await,
            opposite.display.blink_on,
        );

        let page = run(&mut dos, &[0xb8, 1, 5, 0xcd, 0x10, 0xc3]).await;
        assert_eq!(page.display.registers.crtc.display_start, 0x8000);
        let image = gpu.render(&page, [640, 350]).await;
        for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (index % 640, index / 640);
            let level = if (8..16).contains(&x) && (14..28).contains(&y) {
                170
            } else if x == 639 && y == 349 {
                255
            } else {
                0
            };
            assert_eq!(*pixel, [level, level, level, 255], "pixel ({x}, {y})");
        }
        let steady = run(
            &mut dos,
            &[
                0xb8, 0, 5, 0xcd, 0x10, // Display page zero again.
                0xb8, 3, 0x10, 0x31, 0xdb, 0xcd, 0x10, // Disable blink through mono ports.
                0xc3,
            ],
        )
        .await;
        assert_monochrome_pattern(&gpu.render(&steady, [640, 350]).await, true);
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        drop(dos);
        assert_eq!(&gpu.render(&text, [640, 200]).await[..4], &[170, 0, 0, 255]);
        assert_eq!(gpu.render(&first, [640, 350]).await, original);
        assert_monochrome_pattern(
            &gpu.render(&opposite, [640, 350]).await,
            opposite.display.blink_on,
        );
    });
}
