use super::{common, run, Gpu};

#[test]
fn guest_unchains_vram_and_flips_pages_without_rewriting_pixels() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let mut program = vec![
            0xb8, 0x13, 0, 0xcd, 0x10, // BIOS mode 13h.
            0xba, 0xc4, 3, 0xb8, 4, 6, 0xef, // Disable chain-4.
            0xba, 0xd4, 3, 0xb8, 0x14, 0, 0xef, // Disable doubleword addressing.
            0xb8, 0x17, 0xe3, 0xef, // Enable byte addressing.
            0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
            0x31, 0xff, 0x31, 0xc0, // DI = AX = 0.
            0xb9, 0, 0x80, 0xfc, 0xf3, 0xab, // Clear 64 KiB in all four planes.
            0xba, 0xc4, 3, // Sequencer port.
        ];
        for plane in 0..4 {
            program.extend_from_slice(&[0xb8, 2, 1 << plane, 0xef]); // Plane mask.
            program.extend_from_slice(&[0x26, 0xc6, 0x06, 0, 0, plane + 1]); // Page zero.
            program.extend_from_slice(&[0x26, 0xc6, 0x06, 0, 0x40, plane + 9]); // Page one.
        }
        program.push(0xc3);
        let first = run(&mut dos, &program).await;
        let second = run(
            &mut dos,
            &[
                0xba, 0xd4, 3, 0xb8, 12, 0x40, 0xef, // Display start = 4000h.
                0xc3,
            ],
        )
        .await;
        for plane in 0..4 {
            assert!(std::ptr::eq(
                first.display.plane(plane),
                second.display.plane(plane)
            ));
        }
        drop(dos);
        for (capture, colors) in [
            (
                &second,
                [
                    [85, 85, 255, 255],
                    [85, 255, 85, 255],
                    [85, 255, 255, 255],
                    [255, 85, 85, 255],
                ],
            ),
            (
                &first,
                [
                    [0, 0, 170, 255],
                    [0, 170, 0, 255],
                    [0, 170, 170, 255],
                    [170, 0, 0, 255],
                ],
            ),
        ] {
            let pixels = gpu.render(capture, [320, 200]).await;
            assert_eq!(&pixels[..16], colors.as_flattened());
            assert!(pixels[16..]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 0, 255]));
        }
    });
}
