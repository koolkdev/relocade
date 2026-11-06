use super::{common, planar16::COLORS, run, Gpu, TEXT_WITH_RED_BLOCK};

fn colors(runs: &[(u8, usize)]) -> Vec<u8> {
    runs.iter()
        .flat_map(|&(color, count)| std::iter::repeat_n(COLORS[usize::from(color)], count))
        .flatten()
        .collect()
}

#[test]
fn guest_pans_chained_serializers_across_a_split() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let mut program = vec![
            0xb8, 0x0e, 0, 0xcd, 0x10, // Start with BIOS planar graphics.
            0xba, 0xc4, 3, 0xb8, 1, 0x11, 0xef, // Eight dots, load every four clocks.
            0xba, 0xd4, 3, 0xb8, 0x11, 0, 0xef, // Unlock CRTC timing.
        ];
        for (index, value) in [
            (1, 3),
            (2, 255), // 32 active dots, no horizontal blanking inside them.
            (7, 0),
            (9, 0),
            (0x12, 3), // Four scanlines, without repeated rows.
            (8, 0x60), // Byte pan 3.
            (12, 0),
            (13, 0x40), // Upper display starts at 0040h.
            (0x13, 2),  // Four plane bytes per row.
            (0x14, 0x20),
            (0x17, 0xe3), // Byte addresses, counting every four clocks.
            (0x18, 1),    // Lower display starts after scanline 1.
        ] {
            program.extend_from_slice(&[0xb8, index, value, 0xef]);
        }
        program.extend_from_slice(&[
            0xba, 0xda, 3, 0xec, // Reset Attribute Controller access phase.
            0xba, 0xc0, 3, // Attribute Controller.
            0xb0, 0x30, 0xee, 0xb0, 0x21, 0xee, // Reset pan at the split.
            0xb0, 0x33, 0xee, 0xb0, 7, 0xee, // Pel pan 7: total pan is 31 dots.
            0xb0, 0x20, 0xee, // Display enabled.
            0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
            0xba, 0xc4, 3, // Sequencer plane mask.
        ]);
        // Distinct source groups above and below the split, including the load
        // following each row's start. All writes go through guest MMIO.
        for plane in 0..4 {
            program.extend_from_slice(&[0xb8, 2, 1 << plane, 0xef]);
            for (address, color) in [
                (0u16, 9),
                (1, 11),
                (4, 2),
                (5, 5),
                (0x40, 1),
                (0x41, 3),
                (0x44, 4),
                (0x45, 6),
            ] {
                let [low, high] = address.to_le_bytes();
                let byte = if color & (1 << plane) != 0 { 255 } else { 0 };
                program.extend_from_slice(&[0x26, 0xc6, 0x06, low, high, byte]);
            }
        }
        program.push(0xc3);
        let reset_pan = run(&mut dos, &program).await;
        assert_eq!(reset_pan.display.registers.crtc.display_start, 0x40);
        // Each upper row begins at the final dot of a four-clock load.
        let upper = colors(&[
            (2, 1),
            (3, 8),
            (9, 8),
            (12, 8),
            (6, 7),
            (8, 1),
            (6, 8),
            (3, 8),
            (9, 8),
            (12, 7),
        ]);
        let expected_reset = [
            upper.clone(),
            colors(&[
                (9, 8),
                (12, 8),
                (6, 8),
                (3, 8),
                (2, 8),
                (1, 8),
                (8, 8),
                (4, 8),
            ]),
        ]
        .concat();
        assert_eq!(gpu.render(&reset_pan, [32, 4]).await, expected_reset);

        let keep_pan = run(
            &mut dos,
            &[
                0xba, 0xda, 3, 0xec, // Reset Attribute Controller access phase.
                0xba, 0xc0, 3, // Attribute Controller.
                0xb0, 0x30, 0xee, 0xb0, 1, 0xee, // Keep pan below the split.
                0xb0, 0x20, 0xee, 0xc3, // Display enabled; return to DOS.
            ],
        )
        .await;
        for plane in 0..4 {
            assert!(std::ptr::eq(
                reset_pan.display.plane(plane),
                keep_pan.display.plane(plane)
            ));
        }
        let expected_keep = [
            upper,
            colors(&[
                (3, 1),
                (11, 8),
                (13, 8),
                (14, 8),
                (7, 7),
                (4, 1),
                (5, 8),
                (10, 8),
                (5, 8),
                (10, 7),
            ]),
        ]
        .concat();
        assert_eq!(gpu.render(&keep_pan, [32, 4]).await, expected_keep);
        drop(dos);
        assert_eq!(gpu.render(&reset_pan, [32, 4]).await, expected_reset);
    });
}

#[test]
fn guest_combines_shift_load_with_chain_four_then_restores_bios_modes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let mut program = vec![
            0xb8, 0x13, 0, 0xcd, 0x10, // BIOS chain-4 graphics.
            0xba, 0xc4, 3, 0xb8, 1, 5, 0xef, // Load every two clocks.
            0xba, 0xd4, 3, 0xb8, 0x11, 0, 0xef, // Unlock CRTC timing.
        ];
        for (index, value) in [
            (1, 3),
            (2, 255),
            (7, 0x10),
            (9, 0x40),
            (0x12, 0),    // 16 by 1 pixels.
            (0x17, 0xab), // Count every two clocks; retain doubleword addresses.
        ] {
            program.extend_from_slice(&[0xb8, index, value, 0xef]);
        }
        program.extend_from_slice(&[0xb8, 0, 0xa0, 0x8e, 0xc0]);
        for (offset, color) in [1, 2, 4, 8, 3, 6, 12, 9].into_iter().enumerate() {
            program.extend_from_slice(&[0x26, 0xc6, 0x06, offset as u8, 0, color]);
        }
        program.push(0xc3);
        let shifted = run(&mut dos, &program).await;
        let expected: Vec<u8> = [1, 2, 4, 8, 8, 1, 2, 4, 3, 6, 12, 9, 9, 3, 6, 12]
            .into_iter()
            .flat_map(|color| COLORS[color])
            .collect();
        assert_eq!(gpu.render(&shifted, [16, 1]).await, expected);

        // Standard BIOS modes reset both load and count controls. Pixel checks
        // catch lingering serializer state as well as incorrect register presets.
        for (mode, size, byte, white_pixels) in
            [(0x0e, [640, 200], 255, 8), (0x13, [320, 200], 15, 1)]
        {
            let capture = run(
                &mut dos,
                &[
                    0xb8, mode, 0, 0xcd, 0x10, // Set the BIOS mode.
                    0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
                    0x26, 0xc6, 0x06, 0, 0, byte, 0xc3, // First plane byte/pixel.
                ],
            )
            .await;
            let registers = &capture.display.registers;
            assert_eq!(registers.sequencer.values[1] & 0x14, 0);
            assert_eq!(registers.crtc.registers.values[0x14] & 0x20, 0);
            assert_eq!(registers.crtc.registers.values[0x17] & 8, 0);
            let image = gpu.render(&capture, size).await;
            assert_eq!(&image[..white_pixels * 4], &[255; 4].repeat(white_pixels));
            assert!(image[white_pixels * 4..]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 0, 255]));
        }
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        drop(dos);
        let image = gpu.render(&text, [720, 400]).await;
        assert_eq!(&image[..36], &[170, 0, 0, 255].repeat(9));
        assert_eq!(&image[36..40], &[0, 0, 0, 255]);
        assert_eq!(gpu.render(&shifted, [16, 1]).await, expected);
    });
}
