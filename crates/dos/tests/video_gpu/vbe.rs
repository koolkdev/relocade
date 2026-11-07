use super::{common, run, Gpu, TEXT_WITH_RED_BLOCK};
use relocade_machine::FarPointer;
use relocade_x86::{ExecutionExit, HostExit};

#[test]
fn guest_pans_to_a_second_image_with_a_wider_logical_scanline() {
    pollster::block_on(async {
        let mut gpu = Gpu::new().await;
        let mut dos = common::dos().await;
        let mut program = vec![
            0xb8, 2, 0x4f, 0xbb, 3, 1, 0xcd, 0x10, // 800x600 indexed mode.
            0xb8, 6, 0x4f, 0x31, 0xdb, 0xb9, 0, 4, 0xcd, 0x10, 0xa3, 0, 6, // Pitch 1024.
            0xb8, 0x10, 0x10, 0xbb, 1, 0, 0x31, 0xc9, 0xba, 0, 63, 0xcd, 0x10, // Red 1.
            0xb8, 0x10, 0x10, 0xbb, 2, 0, 0xb9, 0, 63, 0x31, 0xd2, 0xcd, 0x10, // Green 2.
            0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000h.
            0x26, 0xc6, 6, 0, 0, 2, // Original viewport: one green pixel.
            0xb8, 5, 0x4f, 0x31, 0xdb, 0xba, 9, 0, 0xcd, 0x10, 0xa3, 2, 6, // Bank 9.
            0x26, 0xc6, 6, 0x18, 0x60, 1, // (24, 600), the panned top-left.
            0x26, 0xc6, 6, 0x37, 0x63, 1, // Top-right.
            0x26, 0xc6, 6, 0x18, 0x64, 1, // Next logical row.
            0xb8, 5, 0x4f, 0xba, 18, 0, 0xcd, 0x10, // Bank 18.
            0x26, 0xc6, 6, 0x37, 0xbf, 1, // Last visible pixel in the panned image.
        ];
        let first_eip = 0x100 + program.len() as u32;
        program.extend([
            0xeb, 0xfe, // Pause before panning.
            0xb8, 7, 0x4f, 0xbb, 0x80, 0, 0xb9, 24, 0, 0xba, 0x58, 2, 0xcd, 0x10, 0xa3, 4,
            6, // Pan to (24, 600) during retrace; save status.
            0xb8, 7, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0xa3, 6, 6, // Query origin.
            0x89, 0x0e, 8, 6, 0x89, 0x16, 10, 6, // Save CX, DX.
            0xb8, 5, 0x4f, 0xbb, 0, 1, 0xcd, 0x10, 0x89, 0x16, 12, 6, // Query bank.
        ]);
        let second_eip = 0x100 + program.len() as u32;
        program.extend([0xeb, 0xfe]);
        dos.launch_com(&program, Default::default()).unwrap();
        let mut captures = Vec::new();
        for (entry, stop, origin) in [
            (0x100, first_eip, [0, 0]),
            (first_eip + 2, second_eip, [24, 600]),
        ] {
            dos.cpu().context().cpu.set_eip(entry);
            assert_eq!(
                common::run_until(dos.cpu(), stop).await,
                ExecutionExit::Interrupted
            );
            let current = dos.frame_source().current_capture().filter(|capture| {
                matches!(capture.display.scanout(), relocade_vga::Scanout::Svga(layout)
                    if layout.pitch() == 1024 && layout.origin() == origin)
                    && capture.display.vram_byte(0) == Some(2)
            });
            captures.push(match current {
                Some(capture) => capture,
                None => common::next_capture(&dos).await,
            });
        }
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in (0x600..0x608).step_by(2) {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x4f);
            }
            for (offset, value) in [(0x608, 24), (0x60a, 600), (0x60c, 18)] {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), value);
            }
        }
        drop(dos);
        for index in [0, 1, 0] {
            let pixels = gpu.render(&captures[index], [800, 600]).await;
            for (offset, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let expected = if index == 0 && offset == 0 {
                    [0, 255, 0, 255]
                } else if index == 1 && [0, 799, 800, 479999].contains(&offset) {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(*pixel, expected, "image {index}, pixel {offset}");
            }
        }
    });
}

#[test]
fn guest_discovers_vbe_restores_banked_graphics_and_returns_to_text() {
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
        let graphics_eip = 0x100 + program.len() as u32;
        program.extend([
            0xeb, 0xfe, // Pause to render the original image.
            0x0e, 0x07, // ES = CS; state buffer at 0800h.
            0xb8, 4, 0x4f, 0xb9, 15, 0, 0x31, 0xd2, 0xcd, 0x10, 0xa3, 0x20, 6, // Query size.
            0x89, 0x1e, 0x30, 6, // Save required 64-byte block count.
            0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb2, 1, 0xcd, 0x10, 0xa3, 0x22, 6, // Save all state.
            0xb8, 2, 0x4f, 0xbb, 3, 0x81, 0xcd, 0x10, 0xa3, 0x24,
            6, // 800x600, preserve VRAM.
        ]);
        let changed_eip = 0x100 + program.len() as u32;
        program.extend([
            0xeb, 0xfe, // Render the changed mode and palette before restoring.
            0xb8, 4, 0x4f, 0xbb, 0, 8, 0xb2, 2, 0xcd, 0x10, 0xa3, 0x26,
            6, // Restore all state.
            0xb8, 3, 0x4f, 0xcd, 0x10, 0xa3, 0x28, 6, // Query restored mode.
            0x89, 0x1e, 0x32, 6, // Save the restored mode number.
            0xb8, 5, 0x4f, 0xbb, 0, 1, 0xcd, 0x10, 0xa3, 0x2a, 6, // Query restored bank.
            0x89, 0x16, 0x34, 6, // Save the restored bank.
            0xb8, 8, 0x4f, 0xbb, 1, 0, 0xcd, 0x10, 0xa3, 0x2c, 6, // Query DAC width.
            0x89, 0x1e, 0x36, 6,
        ]);
        let completion_eip = 0x100 + program.len() as u32;
        program.extend([0xeb, 0xfe, 0xc3]); // JMP $ gives run_until a boundary; RET follows.
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), graphics_eip).await,
            ExecutionExit::Interrupted
        );
        let (oem, modes) = {
            let mut context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in (0x600..0x60e).step_by(2) {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x004f);
            }
            assert_eq!(context.memory.read_u16(base + 0x610).unwrap(), 0x101);
            assert_eq!(context.memory.read_u16(base + 0x612).unwrap(), 4);
            let oem = FarPointer::from_packed(context.memory.read_u32(base + 0x406).unwrap());
            let modes = FarPointer::from_packed(context.memory.read_u32(base + 0x40e).unwrap());
            // Discovery pointers must not depend on retaining the query buffers.
            context.memory.write(base + 0x400, &[0xcc; 512]).unwrap();
            (oem, modes)
        };
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
        dos.cpu().context().cpu.set_eip(graphics_eip + 2);
        assert_eq!(
            common::run_until(dos.cpu(), changed_eip).await,
            ExecutionExit::Interrupted
        );
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in (0x620..0x626).step_by(2) {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x4f);
            }
            assert_eq!(context.memory.read_u16(base + 0x630).unwrap(), 15);
        }
        let changed = match dos.frame_source().current_capture().filter(|capture| {
            capture.display.geometry().size == [800, 600]
                && capture.display.dac.colors[1] != [63, 0, 0]
        }) {
            Some(capture) => capture,
            None => common::next_capture(&dos).await,
        };
        let changed_pixels = gpu.render(&changed, [800, 600]).await;
        assert_ne!(&changed_pixels[65535 * 4..65536 * 4], &[255, 0, 0, 255]);

        dos.cpu().context().cpu.set_eip(changed_eip + 2);
        assert_eq!(
            common::run_until(dos.cpu(), completion_eip).await,
            ExecutionExit::Interrupted
        );
        {
            let context = dos.cpu().context();
            let base = u32::from(context.cpu.snapshot().segments.ds.selector) << 4;
            for offset in (0x626..0x62e).step_by(2) {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), 0x4f);
            }
            for (offset, value) in [(0x632, 0x101), (0x634, 4), (0x636, 0x0601)] {
                assert_eq!(context.memory.read_u16(base + offset).unwrap(), value);
            }
        }
        let restored_graphics = match dos.frame_source().current_capture().filter(|capture| {
            capture.display.scanout() == graphics.display.scanout()
                && capture.display.dac.colors[1] == [63, 0, 0]
        }) {
            Some(capture) => capture,
            None => common::next_capture(&dos).await,
        };
        assert_eq!(gpu.render(&restored_graphics, [640, 480]).await, pixels);
        dos.cpu().context().cpu.set_eip(completion_eip + 2);
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        let text = run(&mut dos, TEXT_WITH_RED_BLOCK).await;
        let restored = gpu.render(&text, [720, 400]).await;
        assert_eq!(&restored[..4], &[170, 0, 0, 255]);
        assert_eq!(&restored[9 * 4..10 * 4], &[0, 0, 0, 255]);
        // The banked capture stays usable after mode 3 overwrites live video memory.
        assert_eq!(gpu.render(&graphics, [640, 480]).await, pixels);
        assert_eq!(gpu.render(&restored_graphics, [640, 480]).await, pixels);
        // The old guest's discovery pointers remain valid after exit and mode changes.
        let context = dos.cpu().context();
        let mut name = [0; 21];
        context.memory.read_wrapping(oem, &mut name).unwrap();
        assert_eq!(&name, b"Relocade banked SVGA\0");
        let mut list = [0; 20];
        context.memory.read_wrapping(modes, &mut list).unwrap();
        assert_eq!(
            list.as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
            [0x101, 0x103, 0x105, 0x111, 0x114, 0x117, 0x142, 0x143, 0x144, 0xffff]
        );
    });
}
