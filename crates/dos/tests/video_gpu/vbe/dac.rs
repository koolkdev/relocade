use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};

#[test]
fn guest_programs_and_restores_eight_bit_palette_colors() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut gpu = Gpu::new().await;
        let original = run(
            &mut dos,
            &[
                0xb8, 2, 0x4f, 0xbb, 1, 1, 0xcd, 0x10, 0xa3, 0, 6, // VBE 101h.
                0xb8, 8, 0x4f, 0xbb, 0, 8, 0xcd, 0x10, 0xa3, 2, 6, // Eight-bit DAC.
                0x89, 0x1e, 8, 6, // Save returned BH/BL.
                0xb8, 0, 0x50, 0x8e, 0xc0, // ES = 5000h.
                0x26, 0xc7, 6, 0, 1, 7, 129, // B/G at 5000:0100.
                0x26, 0xc7, 6, 2, 1, 200, 0xff, // R/padding.
                0xb8, 9, 0x4f, 0x31, 0xdb, 0xb9, 1, 0, 0xba, 1, 0, 0xbf, 0, 1, 0xcd, 0x10, 0xa3, 4,
                6, // Set palette entry 1 through the hooked VBE service.
                0xb8, 0, 0xa0, 0x8e, 0xc0, 0x26, 0xc6, 6, 0, 0,
                1, // First pixel uses entry 1.
                0xb8, 0, 0x50, 0x8e, 0xc0, 0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb9, 15, 0, 0xba, 1, 0,
                0xcd, 0x10, 0xa3, 6, 6, 0xc3, // Save all state at 5000:0800.
            ],
        )
        .await;
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in [0x600, 0x602, 0x604, 0x606] {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x4f);
            }
            assert_eq!(context.memory.read_u16(base + 0x608).unwrap(), 0x0800);
        }
        assert_eq!(original.dac.bits_per_component, 8);
        let pixels = gpu.render(&original, [640, 480]).await;
        assert_eq!(&pixels[..4], &[200, 129, 7, 255]);

        let changed = run(
            &mut dos,
            &[
                0xb8, 2, 0x4f, 0xbb, 3, 0x81, 0xcd, 0x10, // 800×600, preserve VRAM.
                0xb8, 8, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0x89, 0x1e, 0, 6, 0xc3,
            ],
        )
        .await;
        assert_eq!(changed.dac.bits_per_component, 6);
        assert_eq!(changed.geometry().size, [800, 600]);
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            assert_eq!(context.memory.read_u16(base + 0x600).unwrap(), 0x0601);
        }
        let restored = run(
            &mut dos,
            &[
                0xb8, 0, 0x50, 0x8e, 0xc0, 0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb9, 15, 0, 0xba, 2, 0,
                0xcd, 0x10, 0xa3, 0, 6, 0xb8, 8, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0xa3, 2, 6, 0x89,
                0x1e, 6, 6, 0xb8, 9, 0x4f, 0xbb, 1, 0, 0xb9, 1, 0, 0xba, 1, 0, 0xbf, 4, 1, 0xcd,
                0x10, 0xa3, 4, 6, 0xc3, // Read palette entry 1 at 5000:0104.
            ],
        )
        .await;
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in [0x600, 0x602, 0x604] {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x4f);
            }
            assert_eq!(context.memory.read_u16(base + 0x606).unwrap(), 0x0801);
            let mut entry = [0; 4];
            context.memory.read(0x50104, &mut entry).unwrap();
            assert_eq!(entry, [7, 129, 200, 0]);
        }
        assert_eq!(restored.dac.bits_per_component, 8);
        assert_eq!(gpu.render(&restored, [640, 480]).await, pixels);
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        assert_eq!(text.dac.bits_per_component, 6);
        assert_eq!(&gpu.render(&text, [720, 400]).await[..4], &[170, 0, 0, 255]);
        drop(dos);
        for capture in [&original, &restored] {
            assert_eq!(gpu.render(capture, [640, 480]).await, pixels);
        }
    });
}
