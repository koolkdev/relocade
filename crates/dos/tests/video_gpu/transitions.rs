use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};

fn assert_white_prefix(image: &[u8], count: usize) {
    for (index, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let expected = if index < count {
            [255; 4]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "pixel {index}");
    }
}

#[test]
fn blanking_and_bios_mode_changes_replace_scrolled_state_without_invalidating_captures() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let scrolled = run(
            &mut dos,
            &[
                0xb8, 0x13, 0, 0xcd, 0x10, // BIOS mode 13h.
                0xba, 0xc4, 3, 0xb8, 4, 6, 0xef, // Unchain all four planes.
                0xba, 0xd4, 3, 0xb8, 0x14, 0, 0xef, // Disable doubleword addresses.
                0xb8, 0x17, 0xe3, 0xef, // Byte addresses.
                0xb8, 0x11, 0x0e, 0xef, // Unlock timing registers.
                0xb8, 7, 0x0f, 0xef, // Clear line-compare bit 8.
                0xb8, 9, 1, 0xef, // Repeat rows twice; clear line-compare bit 9.
                0xb8, 12, 0x40, 0xef, // Display start = 4000h.
                0xb8, 8, 0x21, 0xef, // One preset row and one byte of panning.
                0xb8, 0x18, 1, 0xef, // Split after physical scanline 1.
                0xba, 0xda, 3, 0xec, // Reset Attribute Controller access phase.
                0xba, 0xc0, 3, 0xb0, 0x30, 0xee, 0xb0, 0x61, 0xee, // Reset pan at split.
                0xb0, 0x33, 0xee, 0xb0, 6, 0xee, // Three pixels of pel panning.
                0xb0, 0x20, 0xee, // Display enabled.
                0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                // Mode X exposes plane memory beyond the BIOS mode-13h clear.
                0x31, 0xff, 0x31, 0xc0, // DI = AX = 0.
                0xb9, 0, 0x80, 0xfc, 0xf3, 0xab, // Clear all four complete planes.
                0x26, 0xc6, 0x06, 1, 0x40, 15, // Four white pixels at plane offset 4001h.
                0xc3,
            ],
        )
        .await;
        assert_eq!(scrolled.display.registers.crtc.display_start, 0x4000);
        // The preset keeps physical rows. Panning exposes only the fourth white
        // pixel, and the lower split starts at cleared memory.
        assert_white_prefix(&gpu.render(&scrolled, [320, 400]).await, 1);

        let blanked = run(&mut dos, &[0xba, 0xc4, 3, 0xb8, 1, 0x21, 0xef, 0xc3]).await;
        assert_white_prefix(&gpu.render(&blanked, [320, 400]).await, 0);
        let restored = run(&mut dos, &[0xba, 0xc4, 3, 0xb8, 1, 1, 0xef, 0xc3]).await;
        assert_white_prefix(&gpu.render(&restored, [320, 400]).await, 1);
        for plane in 0..4 {
            assert!(std::ptr::eq(
                scrolled.display.plane(plane),
                blanked.display.plane(plane)
            ));
            assert!(std::ptr::eq(
                scrolled.display.plane(plane),
                restored.display.plane(plane)
            ));
        }

        let mut retained = Vec::new();
        for (mode, size) in [(0x12, [640, 480]), (0x0d, [320, 200])] {
            let capture = run(
                &mut dos,
                &[
                    0xb8, mode, 0, 0xcd, 0x10, // Set the next BIOS mode.
                    0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                    0x26, 0xc6, 0x06, 0, 0, 0xff, // Eight white planar pixels.
                    0xc3,
                ],
            )
            .await;
            assert_eq!(capture.display.registers.crtc.display_start, 0);
            assert_white_prefix(&gpu.render(&capture, size).await, 8);
            retained.push((capture, size));
        }
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        drop(dos);
        let pixels = gpu.render(&text, [640, 200]).await;
        assert_eq!(&pixels[..32], &[170, 0, 0, 255].repeat(8));
        assert_eq!(&pixels[32..36], &[0, 0, 0, 255]);
        for (capture, size) in retained.into_iter().rev() {
            assert_white_prefix(&gpu.render(&capture, size).await, 8);
        }
        assert_white_prefix(&gpu.render(&scrolled, [320, 400]).await, 1);
    });
}
