use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};
use relocade_x86::{ExecutionExit, HostExit};

#[test]
fn guest_discovers_vbe_draws_across_banks_and_returns_to_text() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let mut program = vec![
            0x0e, 0x07, // ES = CS; controller buffer at 0400h.
            0xbf, 0, 4, 0xb8, 0, 0x4f, 0xcd, 0x10, 0xa3, 0, 6, // Controller information.
            0xc4, 0x36, 0x0e, 4, // LES SI,[040Eh]: follow the mode-list pointer.
            0x26, 0x8b, 0x1c, // BX = ES:[SI]: first advertised mode.
            0x89, 0xd9, 0x0e, 0x07, // CX = BX; ES = CS.
            0xbf, 0, 5, 0xb8, 1, 0x4f, 0xcd, 0x10, 0xa3, 2, 6, // Mode information.
            0xb8, 2, 0x4f, 0xcd, 0x10, 0xa3, 4, 6, // Set discovered mode.
            0xb8, 0x10, 0x10, 0xbb, 1, 0, // BIOS DAC entry 1 = full red.
            0xb9, 0, 0, 0xba, 0, 63, 0xcd, 0x10, // CH/CL = green/blue, DH = red.
            0xa1, 8, 5, 0x8e, 0xc0, // ES = mode-info WinASegment.
            0x26, 0xc6, 6, 0xff, 0xff, 1, // Last byte in bank 0.
            0xb8, 5, 0x4f, 0x31, 0xdb, 0xba, 1, 0, 0xcd, 0x10, 0xa3, 6, 6, // Select bank 1.
            0x26, 0xc6, 6, 0, 0, 1, // First byte in bank 1.
            0xb8, 5, 0x4f, 0xba, 4, 0, 0xcd, 0x10, 0xa3, 8, 6, // Select bank 4.
            0x26, 0xc6, 6, 0xff, 0xaf, 1, // Last visible pixel, in bank 4.
            0xb8, 3, 0x4f, 0xcd, 0x10, 0xa3, 0x0a, 6, 0x89, 0x1e, 0x10, 6, // Query mode.
            0xb8, 5, 0x4f, 0xbb, 0, 1, 0xcd, 0x10, 0xa3, 0x0c, 6, // Query window A.
            0x89, 0x16, 0x12, 6, // Save queried bank.
        ];
        let completion_eip = 0x100 + program.len() as u32;
        program.extend([0xeb, 0xfe, 0xc3]); // JMP $ gives run_until a boundary; RET follows.
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), completion_eip).await,
            ExecutionExit::Interrupted
        );
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in (0x600..0x60e).step_by(2) {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x004f);
            }
            assert_eq!(context.memory.read_u16(base + 0x610).unwrap(), 0x101);
            assert_eq!(context.memory.read_u16(base + 0x612).unwrap(), 4);
        }
        // The trailing BIOS queries may already have published the final write.
        // A static packed frame will not publish again without a change.
        let graphics = match dos
            .frame_source()
            .current_capture()
            .filter(|capture| capture.display.vram_byte(307199) == Some(1))
        {
            Some(capture) => capture,
            None => common::next_capture(&dos).await,
        };
        let pixels = gpu.render(&graphics, [640, 480]).await;
        for (offset, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let expected = if [65535, 65536, 307199].contains(&offset) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(*pixel, expected, "VBE guest pixel {offset}");
        }
        dos.cpu().context().cpu.set_eip(completion_eip + 2);
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        let restored = gpu.render(&text, [720, 400]).await;
        assert_eq!(&restored[..4], &[170, 0, 0, 255]);
        assert_eq!(&restored[9 * 4..10 * 4], &[0, 0, 0, 255]);
        // The banked capture stays usable after mode 3 overwrites live video memory.
        assert_eq!(gpu.render(&graphics, [640, 480]).await, pixels);
    });
}
