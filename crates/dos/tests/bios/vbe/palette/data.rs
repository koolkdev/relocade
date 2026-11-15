use super::{common, video, FarPointer, Segment, SegmentRegister, VideoInspection, PROGRAM};
use relocade_dos::Dos;

fn buffer(dos: &mut Dos, pointer: FarPointer) {
    let context = dos.cpu().context();
    context.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, pointer.segment),
    );
    context.cpu.set_di(pointer.offset);
}

#[test]
fn palette_blocks_use_bgr_entries_and_share_the_legacy_dac() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_eq!(video(&mut dos, 0x4f02, 0x103, 0, 0).registers.eax, 0x4f);
        video(&mut dos, 0x4f05, 0, 0, 7);
        let pointer = FarPointer {
            segment: 0x5000,
            offset: 0xfffd,
        };
        buffer(&mut dos, pointer);
        {
            let mut context = dos.cpu().context();
            context.cpu.set_si(0x1234);
            context.cpu.set_bp(0x5678);
            context
                .memory
                .write_wrapping(pointer.wrapping_add(0xffff), &[0xa5; 10])
                .unwrap();
            context
                .memory
                .write_wrapping(pointer, &[0xff, 0x80, 0x41, 0xee, 34, 51, 4, 0xdd])
                .unwrap();
            context.memory.write_u8(0xa0010, 0x7b).unwrap();
            context.io.write_port(0x3c6, 1, 0x5a);
            context.io.write_port(0x1ce, 2, 0x1234);
        }
        let result = video(&mut dos, 0x4f09, 0xa500, 2, 254);
        assert_eq!(result.registers.eax, 0x4f);
        assert_eq!(
            [
                result.registers.ebx,
                result.registers.ecx,
                result.registers.edx
            ],
            [0xa500, 2, 254]
        );
        assert_eq!(
            [
                result.registers.esi,
                result.registers.edi,
                result.registers.ebp
            ],
            [0x1234, 0xfffd, 0x5678]
        );
        assert_eq!(result.segments.es.selector, pointer.segment);
        {
            let vga = dos.vga();
            let card = vga.borrow();
            assert_eq!(
                &card.registers().dac.colors[254..],
                &[[1, 0, 63], [4, 51, 34]]
            );
            assert_eq!(card.registers().dac.address(), 0);
            assert_eq!(card.registers().dac.mask, 0x5a);
        }
        let dac = dos.vga().borrow().dac_state();
        dos.cpu()
            .context()
            .memory
            .write_wrapping(pointer, &[0xcc; 8])
            .unwrap();
        let result = video(&mut dos, 0x4f09, 0xa501, 2, 254);
        assert_eq!(result.registers.eax, 0x4f);
        assert_eq!(
            [
                result.registers.ebx,
                result.registers.ecx,
                result.registers.edx
            ],
            [0xa501, 2, 254]
        );
        let mut bytes = [0; 10];
        dos.cpu()
            .context()
            .memory
            .read_wrapping(pointer.wrapping_add(0xffff), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0xa5, 63, 0, 1, 0, 34, 51, 4, 0, 0xa5]);
        assert_eq!(dos.vga().borrow().dac_state(), dac);
        assert_eq!(dos.cpu().context().io.read_port(0x1ce, 2), 0x1234);
        assert_eq!(video(&mut dos, 0x4f05, 0x100, 0, 0).registers.edx, 7);
        assert_eq!(dos.cpu().context().memory.read_u8(0xa0010).unwrap(), 0x7b);
        let legacy = video(&mut dos, 0x1015, 254, 0, 0);
        assert_eq!(legacy.registers.ecx, 63);
        assert_eq!(legacy.registers.edx, 0x100);

        // The maximum transfer addresses all 256 entries, including the last.
        let entries: Vec<u8> = (0..256u16)
            .flat_map(|index| [index as u8 & 63, 17, 42, 0xff])
            .collect();
        dos.cpu()
            .context()
            .memory
            .write_wrapping(pointer, &entries)
            .unwrap();
        assert_eq!(video(&mut dos, 0x4f09, 0, 256, 0).registers.eax, 0x4f);
        for (index, color) in dos.vga().borrow().registers().dac.colors.iter().enumerate() {
            assert_eq!(*color, [42, 17, index as u8 & 63]);
        }
    });
}

#[test]
fn invalid_and_empty_palette_transfers_preserve_device_and_buffer_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let pointer = FarPointer {
            segment: 0x5000,
            offset: 0x200,
        };
        buffer(&mut dos, pointer);
        {
            let mut context = dos.cpu().context();
            context.memory.write_wrapping(pointer, &[0xa5; 16]).unwrap();
            context.io.write_port(0x3c8, 1, 17);
            context.io.write_port(0x3c9, 1, 23); // Leave a write at green.
        }
        let dac = dos.vga().borrow().dac_state();
        let core = dos.vga().borrow().core_state();
        for (operation, count, first, status) in [
            (0, 2, 255, 0x14f),
            (1, 257, 0, 0x14f),
            (0, 1, 256, 0x14f),
            (1, 0, 256, 0x14f),
            (0, 0xffff, 2, 0x14f),
            (4, 1, 0, 0x14f),
            (0x81, 1, 0, 0x14f),
            (2, 1, 0, 0x24f),
            (3, 1, 0, 0x24f),
            (0, 0, 255, 0x4f),
            (1, 0, 0, 0x4f),
            (0x80, 0, 17, 0x4f),
        ] {
            let result = video(&mut dos, 0x4f09, operation, count, first);
            assert_eq!(result.registers.eax, status);
            assert_eq!(
                [
                    result.registers.ebx,
                    result.registers.ecx,
                    result.registers.edx
                ],
                [u32::from(operation), u32::from(count), u32::from(first)]
            );
            assert_eq!(dos.vga().borrow().dac_state(), dac);
            assert_eq!(dos.vga().borrow().core_state(), core);
        }
        // Fixed direct-color ramps cannot be programmed as indexed palettes.
        video(&mut dos, 0x4f02, 0x111, 0, 0);
        for operation in [0, 1, 0x80] {
            assert_eq!(
                video(&mut dos, 0x4f09, operation, 1, 17).registers.eax,
                0x34f
            );
            assert_eq!(dos.vga().borrow().dac_state(), dac);
        }
        dos.cpu().context().io.write_port(0x46e8, 1, 0);
        for operation in [0, 1, 0x80] {
            assert_eq!(
                video(&mut dos, 0x4f09, operation, 1, 17).registers.eax,
                0x14f
            );
            assert_eq!(dos.vga().borrow().dac_state(), dac);
        }
        let mut bytes = [0; 16];
        dos.cpu()
            .context()
            .memory
            .read_wrapping(pointer, &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0xa5; 16]);
    });
}
