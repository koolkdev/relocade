use super::{buffer, state};
use crate::{
    common,
    video::{video, PROGRAM},
};

#[test]
fn query_counts_selected_blocks_and_rejects_empty_or_unknown_requests() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mask, blocks) in [(1, 2), (2, 2), (3, 3), (4, 13), (5, 14), (6, 14), (7, 15)] {
            let result = state(&mut dos, 0, mask | 0x8000, 0x5000, 0x200);
            assert_eq!(result.registers.eax as u16, 0x1c1c);
            assert_eq!(result.registers.ebx as u16, blocks);
        }
        let before = dos.vga().borrow().snapshot();
        dos.cpu()
            .context()
            .memory
            .write(0x50200, &[0xa5; 1024])
            .unwrap();
        for (operation, mask) in [(0, 0), (0, 0x8000), (1, 0), (2, 0), (3, 7), (0xff, 7)] {
            let result = state(&mut dos, operation, mask, 0x5000, 0x200);
            assert_eq!(result.registers.eax as u16, 0x1c00);
            assert_eq!(result.registers.ebx as u16, 0x200);
        }
        assert_eq!(buffer::<1024>(&mut dos, 0x5000, 0x200), [0xa5; 1024]);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
    });
}

#[test]
fn save_uses_absolute_component_pointers_and_preserves_unused_header_and_padding() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mask, pointers, end) in [
            (1, [0x0220u16, 0xa5a5, 0xa5a5], 0x266),
            (2, [0xa5a5, 0x0220, 0xa5a5], 0x25a),
            (4, [0xa5a5, 0xa5a5, 0x0220], 0x524),
            (7, [0x0220, 0x0266, 0x02a0], 0x5a4),
        ] {
            dos.cpu()
                .context()
                .memory
                .write(0x501ff, &[0xa5; 962])
                .unwrap();
            let result = state(&mut dos, 1, mask, 0x5000, 0x200);
            assert_eq!(result.registers.eax as u16, 0x1c1c);
            assert_eq!(
                buffer::<6>(&mut dos, 0x5000, 0x200),
                pointers.map(u16::to_le_bytes).concat().as_slice()
            );
            assert_eq!(buffer::<26>(&mut dos, 0x5000, 0x206), [0xa5; 26]);
            assert_eq!(buffer::<1>(&mut dos, 0x5000, 0x1ff), [0xa5]);
            assert_eq!(buffer::<1>(&mut dos, 0x5000, end), [0xa5]);
        }
        let expected = buffer::<0x46>(&mut dos, 0x5000, 0x220);
        // Header pointer and hardware payload each cross ES:FFFF independently.
        for offset in [0xffff, 0xffc0] {
            state(&mut dos, 1, 1, 0x5000, offset);
            assert_eq!(
                buffer::<2>(&mut dos, 0x5000, offset),
                offset.wrapping_add(0x20).to_le_bytes()
            );
            assert_eq!(
                buffer::<0x46>(&mut dos, 0x5000, offset.wrapping_add(0x20)),
                expected
            );
        }
    });
}

#[test]
fn firmware_save_includes_equipment_and_vectors_before_overlapping_output() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0x410, 0xb7).unwrap();
            context
                .memory
                .write(0x449, &(1..=30).collect::<Vec<_>>())
                .unwrap();
            context
                .memory
                .write(0x484, &[31, 32, 33, 34, 35, 36, 37])
                .unwrap();
            for (address, bytes) in [
                (0x4a8, [38, 39, 40, 41]),
                (0x14, [42, 43, 44, 45]),
                (0x74, [46, 47, 48, 49]),
                (0x7c, [50, 51, 52, 53]),
                (0x10c, [54, 55, 56, 57]),
            ] {
                context.memory.write(address, &bytes).unwrap();
            }
        }
        let before = dos.vga().borrow().snapshot();
        let mut expected = std::array::from_fn::<_, 0x3a, _>(|index| index as u8);
        expected[0] = 0x30;
        state(&mut dos, 1, 2, 0, 0x449);
        assert_eq!(buffer::<0x3a>(&mut dos, 0, 0x469), expected);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
    });
}

#[test]
fn hardware_save_reads_monochrome_registers_and_spills_all_four_latches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x12, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            for (plane, byte) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
                context.io.write_port(0x3c4, 2, ((1 << plane) << 8) | 2);
                context.memory.write_u8(0xa0010, byte).unwrap();
            }
            context.memory.read_u8(0xa0010).unwrap();
            context.io.write_port(0x3c2, 1, 0xe0); // Monochrome ports, RAM disabled.
            context.io.write_port(0x3b4, 1, 0x0e);
            context.io.write_port(0x3ba, 1, 0x55);
            context.io.write_port(0x3ce, 2, 0x0204);
            context.io.write_port(0x3ce, 2, 0x0305);
            context.io.read_port(0x3ba, 1);
            context.io.write_port(0x3c0, 1, 0x2a);
        }
        let before = dos.vga().borrow().snapshot();
        state(&mut dos, 1, 1, 0x5000, 0x200);
        let bytes = buffer::<0x46>(&mut dos, 0x5000, 0x220);
        assert_eq!(&bytes[..5], &[2, 0x0e, 5, 0x2a, 0x55]);
        assert_eq!(&bytes[5..9], &before.registers.sequencer.values[1..]);
        assert_eq!(bytes[9], 0xe0);
        assert_eq!(&bytes[0x0a..0x23], &before.registers.crtc.values);
        assert_eq!(&bytes[0x23..0x37], &before.registers.attribute.values[..20]);
        assert_eq!(&bytes[0x37..0x40], &before.registers.graphics.values);
        assert_eq!(&bytes[0x40..], &[0xb4, 3, 0x12, 0x34, 0x56, 0x78]);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for (plane, byte) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
            assert_eq!(
                &after.plane(plane)[..0xffff],
                &before.plane(plane)[..0xffff]
            );
            assert_eq!(after.plane(plane)[0xffff], byte);
            assert_eq!(before.plane(plane)[0xffff], 0); // Retained capture is immutable.
        }
        let mut context = dos.cpu().context();
        context.io.write_port(0x3c2, 1, 0xe2);
        context.io.write_port(0x3c4, 2, 0x0f02);
        context.io.write_port(0x3ce, 2, 0x0105);
        context.memory.write_u8(0xa0020, 0).unwrap();
        let after_write = dos.vga().borrow().snapshot();
        for (plane, byte) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
            assert_eq!(after_write.plane(plane)[0x20], byte);
        }
    });
}

#[test]
fn dac_save_preserves_all_colors_color_select_and_read_or_write_position() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c8, 1, 0);
            for component in 0..768 {
                context.io.write_port(0x3c9, 1, (component * 7 + 3) & 63);
            }
            context.io.write_port(0x3c6, 1, 0x5a);
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x14);
            context.io.write_port(0x3c0, 1, 0x0d);
        }
        for (port, index, mode) in [(0x3c7, 255, 1), (0x3c8, 42, 0)] {
            {
                let context = dos.cpu().context();
                context.io.write_port(port, 1, index);
                if mode == 1 {
                    context.io.read_port(0x3c9, 1);
                } else {
                    context.io.write_port(0x3c9, 1, (index * 21 + 3) & 63);
                }
            }
            state(&mut dos, 1, 4, 0x5000, 0x200);
            let bytes = buffer::<0x304>(&mut dos, 0x5000, 0x220);
            assert_eq!(&bytes[..3], &[mode, index as u8, 0x5a]);
            for (index, &component) in bytes[3..0x303].iter().enumerate() {
                assert_eq!(component, ((index * 7 + 3) & 63) as u8);
            }
            assert_eq!(bytes[0x303], 13);
            let context = dos.cpu().context();
            assert_eq!(context.io.read_port(0x3c7, 1) & 1, u32::from(mode));
            assert_eq!(
                context.io.read_port(0x3c8, 1),
                u32::from((index as u8).wrapping_add(mode))
            );
            // The interrupted transfer was at green; the BIOS restarts at red.
            if mode == 1 {
                assert_eq!(context.io.read_port(0x3c9, 1), (index * 21 + 3) & 63);
            } else {
                context.io.write_port(0x3c9, 1, 62);
                assert_eq!(
                    dos.vga().borrow().snapshot().registers.dac.colors[42],
                    [62, 60, 3]
                );
            }
        }
    });
}
