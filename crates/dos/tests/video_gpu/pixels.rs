use super::{common, run, Gpu};

#[test]
fn bios_pixel_services_feed_the_captured_gpu_image() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        for (mode, size, page) in [
            (0x0d, [320, 200], 0),
            (0x12, [640, 480], 255),
            (0x13, [320, 200], 255),
        ] {
            let program = [
                0xb8, mode, 0, 0xcd, 0x10, // Set mode.
                0xbb, 0, page, // BH is ignored in single-page modes.
                0xb9, 7, 0, 0xba, 1, 0, // Pixel (7,1).
                0xb8, 15, 0x0c, 0xcd, 0x10, // Write white.
                0xb8, 0, 0x0d, 0xcd, 0x10, // Read color into AL.
                0xa2, 0, 2, // Save AL at DS:0200.
                0xb9, 8, 0, 0xb8, 4, 0x0c, 0xcd, 0x10, // Red next pixel.
                0xc3,
            ];
            let capture = run(&mut dos, &program).await;
            {
                let context = dos.cpu().context();
                let segment = context.cpu.segments().ds.base;
                assert_eq!(context.memory.read_u8(segment + 0x200).unwrap(), 15);
            }
            let pixels = gpu.render(&capture, size).await;
            for (offset, actual) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let expected = match offset as u32 {
                    p if p == size[0] + 7 => [255; 4],
                    p if p == size[0] + 8 => [170, 0, 0, 255],
                    _ => [0, 0, 0, 255],
                };
                assert_eq!(*actual, expected, "mode {mode:x}, pixel {offset}");
            }
        }
    });
}
