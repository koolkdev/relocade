use super::{common, run, ExecutionExit, Gpu, HostExit, TEXT_WITH_RED_BLOCK};
use relocade_vga::{PixelFormat, Scanout};

#[test]
fn guest_draws_pans_and_restores_both_direct_color_formats() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        for (mode, pixel_bytes, values) in [
            (0x111u16, 2u32, [0xf800u32, 0x07e0, 0x001f]),
            (0x142, 4, [0xa5ff_0000, 0x0000_ff00, 0x0100_00ff]),
        ] {
            let mut dos = common::dos().await;
            let format = if pixel_bytes == 2 {
                PixelFormat::Rgb565
            } else {
                PixelFormat::Bgrx8888
            };
            let bank_rows = 65536 / (1024 * pixel_bytes);
            let points = [
                (0, 0, 0),
                (639, bank_rows - 1, 1),
                (0, bank_rows, 2),
                (639, 479, 0),
            ];
            let mut program = vec![0xb8, 2, 0x4f, 0xbb]; // Set direct-color mode.
            program.extend(mode.to_le_bytes());
            program.extend([
                0xcd, 0x10, 0xa3, 0, 6, 0xb8, 6, 0x4f, 0x31, 0xdb, 0xb9, 0, 4, 0xcd,
                0x10, // Width 1024 pixels.
                0xa3, 2, 6, 0x89, 0x1e, 4, 6, 0x89, 0x0e, 6, 6, // Save AX, BX, CX.
                0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000h.
            ]);
            for (index, (x, y, color)) in points.into_iter().enumerate() {
                // Draw into the viewport that starts at pixel (384, 32).
                let address = ((32 + y) * 1024 + 384 + x) * pixel_bytes;
                program.extend([0xb8, 5, 0x4f, 0x31, 0xdb, 0xba]); // Select window A bank.
                program.extend(((address >> 16) as u16).to_le_bytes());
                program.extend([0xcd, 0x10, 0xa3, 0x30 + index as u8 * 2, 6]);
                // Byte stores exercise partial pixels, including the bank's last pixel.
                for (byte, value) in values[color].to_le_bytes()[..pixel_bytes as usize]
                    .iter()
                    .copied()
                    .enumerate()
                {
                    program.extend([0x26, 0xc6, 6]); // MOV byte ES:[offset],value.
                    program.extend((address as u16 + byte as u16).to_le_bytes());
                    program.push(value);
                }
            }
            program.extend([
                0xb8, 7, 0x4f, 0xbb, 0x80, 0, 0xb9, 0x80, 1, 0xba, 32, 0, 0xcd, 0x10, 0xa3, 8,
                6, // Pan to (384, 32) at retrace.
                0x0e, 0x07, // ES = CS; state buffer at 0800h.
                0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb9, 15, 0, 0xba, 1, 0, 0xcd, 0x10, 0xa3, 10,
                6, // Save all state classes.
            ]);
            let graphics_eip = 0x100 + program.len() as u32;
            program.extend([
                0xeb, 0xfe, 0xb8, 2, 0x4f, 0xbb, 1, 0x81, 0xcd, 0x10, 0xa3, 12,
                6, // Indexed, preserve VRAM.
            ]);
            let changed_eip = 0x100 + program.len() as u32;
            program.extend([
                0xeb, 0xfe, 0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb9, 15, 0, 0xba, 2, 0, 0xcd, 0x10, 0xa3,
                14, 6, // Restore direct-color state.
                0xb8, 3, 0x4f, 0xcd, 0x10, 0xa3, 16, 6, 0x89, 0x1e, 0x20, 6, // Current mode.
                0xb8, 6, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0xa3, 18, 6, 0x89, 0x1e, 0x22, 6, 0x89,
                0x0e, 0x24, 6, // Byte pitch and pixel width.
                0xb8, 7, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0xa3, 20, 6, 0x89, 0x0e, 0x26, 6, 0x89,
                0x16, 0x28, 6, // Viewport origin.
                0xb8, 5, 0x4f, 0xbb, 0, 1, 0xcd, 0x10, 0xa3, 22, 6, 0x89, 0x16, 0x2a,
                6, // CPU bank.
            ]);
            let restored_eip = 0x100 + program.len() as u32;
            program.extend([0xeb, 0xfe, 0xc3]);
            dos.launch_com(&program, Default::default()).unwrap();
            let mut captures = Vec::new();
            for (entry, stop, expected_format, origin) in [
                (0x100, graphics_eip, format, [384, 32]),
                (graphics_eip + 2, changed_eip, PixelFormat::Indexed8, [0, 0]),
                (changed_eip + 2, restored_eip, format, [384, 32]),
            ] {
                dos.cpu().context().cpu.set_eip(entry);
                assert_eq!(
                    common::run_until(dos.cpu(), stop).await,
                    ExecutionExit::Interrupted
                );
                let current = dos.frame_source().current_capture().filter(|capture| {
                    matches!(capture.display.scanout(), Scanout::Svga(layout)
                        if layout.format() == expected_format && layout.origin() == origin)
                });
                captures.push(match current {
                    Some(capture) => capture,
                    None => common::next_capture(&dos).await,
                });
            }
            {
                let context = dos.cpu().context();
                let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
                for offset in [
                    0x600, 0x602, 0x608, 0x60a, 0x60c, 0x60e, 0x610, 0x612, 0x614, 0x616, 0x630,
                    0x632, 0x634, 0x636,
                ] {
                    assert_eq!(
                        context.memory.read_u16(base + offset).unwrap(),
                        0x4f,
                        "status at {offset:x}"
                    );
                }
                for (offset, value) in [
                    (0x604, 1024 * pixel_bytes as u16),
                    (0x606, 1024),
                    (0x620, mode),
                    (0x622, 1024 * pixel_bytes as u16),
                    (0x624, 1024),
                    (0x626, 384),
                    (0x628, 32),
                    (0x62a, 8 * pixel_bytes as u16 - 1),
                ] {
                    assert_eq!(
                        context.memory.read_u16(base + offset).unwrap(),
                        value,
                        "result at {offset:x}"
                    );
                }
            }
            let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
            let original = gpu.render(&captures[0], [640, 480]).await;
            for (offset, actual) in original.as_chunks::<4>().0.iter().enumerate() {
                let expected = points
                    .iter()
                    .find(|&&(x, y, _)| y * 640 + x == offset as u32)
                    .map_or([0, 0, 0, 255], |&(_, _, color)| colors[color]);
                assert_eq!(*actual, expected, "mode {mode:x}, pixel {offset}");
            }
            let changed = gpu.render(&captures[1], [640, 480]).await;
            assert_eq!(&changed[..4], &[0, 0, 0, 255]);
            assert_eq!(gpu.render(&captures[2], [640, 480]).await, original);
            dos.cpu().context().cpu.set_eip(restored_eip + 2);
            assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
            let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
            assert_eq!(&gpu.render(&text, [720, 400]).await[..4], &[170, 0, 0, 255]);
            drop(dos);
            // Both captures survive legacy font/VRAM writes and device destruction.
            for capture in [&captures[0], &captures[2]] {
                assert_eq!(gpu.render(capture, [640, 480]).await, original);
            }
        }
    });
}
