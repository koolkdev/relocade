use super::{common, run, Gpu};

#[test]
fn bios_palette_changes_recolor_text_and_graphics_without_vram_writes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        for (mode, size) in [(3, [640, 200]), (0x12, [640, 480]), (0x13, [320, 200])] {
            let mut program = vec![0xb8, mode, 0, 0xcd, 0x10];
            if mode == 3 {
                program.extend_from_slice(&[
                    0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
                    0xb8, 0xdb, 9, 0xbb, 4, 0, 0xb9, 1, 0, 0xcd, 0x10, // Red block.
                ]);
            } else {
                program.extend_from_slice(&[
                    0xb8, 4, 0x0c, 0x31, 0xdb, 0x31, 0xc9, 0x31, 0xd2, 0xcd,
                    0x10, // Red pixel.
                ]);
            }
            program.push(0xc3);
            let original = run(&mut dos, &program).await;
            let red = gpu.render(&original, size).await;
            assert_eq!(&red[..4], &[170, 0, 0, 255]);

            let mut program = Vec::new();
            let index = if mode == 0x13 { 4 } else { 42 };
            if mode != 0x13 {
                program.extend_from_slice(&[
                    0xb8, 0, 0x10, 0xbb, 4, 42, 0xcd, 0x10, // Attribute color 4 -> DAC 42.
                ]);
            }
            program.extend_from_slice(&[
                0xb8, 0x10, 0x10, 0xbb, index, 0, 0xb9, 0, 63, 0x31, 0xd2, 0xcd, 0x10,
                0xc3, // DAC color becomes green.
            ]);
            let recolored = run(&mut dos, &program).await;
            for plane in 0..4 {
                assert!(std::ptr::eq(
                    original.display.plane(plane),
                    recolored.display.plane(plane)
                ));
            }
            assert_eq!(&gpu.render(&recolored, size).await[..4], &[0, 255, 0, 255]);
            let masked = run(
                &mut dos,
                &[
                    0xb8, 0x10, 0x10, 0x31, 0xdb, 0xb9, 63, 0, 0x31, 0xd2, 0xcd,
                    0x10, // DAC 0 blue.
                    0xb8, 0x18, 0x10, 0x31, 0xdb, 0xcd, 0x10, // Mask every color to zero.
                    0xc3,
                ],
            )
            .await;
            let blue = gpu.render(&masked, size).await;
            assert!(blue
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 255, 255]));
            assert_eq!(gpu.render(&original, size).await, red);
        }
    });
}
