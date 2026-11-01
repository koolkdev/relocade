use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn monochrome_planar_palette_ports_and_no_clear_preservation() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x000f, 0, 0, 0);
        let initial = dos.vga().borrow().snapshot();
        assert_eq!(initial.registers.miscellaneous, 0xa2);
        assert_eq!(initial.registers.attribute.values[0x10], 0x0b);
        assert_eq!(initial.registers.attribute.values[0x12], 5);
        assert_eq!(
            &initial.registers.attribute.values[..16],
            &[0, 8, 0, 0, 24, 24, 0, 0, 0, 8, 0, 0, 0, 24, 0, 0]
        );
        for (index, level) in [(0, 0), (8, 42), (24, 63)] {
            assert_eq!(initial.registers.dac.colors[index], [level; 3]);
        }
        {
            let mut context = dos.cpu().context();
            assert_eq!(context.io.read_port(0x3d5, 1), 255);
            context.io.write_port(0x3b4, 1, 4);
            assert_eq!(context.io.read_port(0x3b5, 1), 0x54);
            // The display mask does not discard CPU writes to any plane or page.
            for plane in 0..4 {
                context.io.write_port(0x3c4, 2, (1 << (plane + 8)) | 2);
                context
                    .memory
                    .write(0xa0000, &vec![0x31 + plane as u8; 65536])
                    .unwrap();
            }
        }
        let before = dos.vga().borrow().snapshot();
        // Mono mode's CGA color call is a no-op, but VGA palette calls use 3BA.
        video(&mut dos, 0x0b00, 0x001f, 0, 0);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        video(&mut dos, 0x1000, 0x1801, 0, 0);
        video(&mut dos, 0x1010, 24, 0, 0);
        video(&mut dos, 0x1003, 0, 0, 0);
        let modified = dos.vga().borrow().snapshot();
        assert_eq!(modified.registers.attribute.values[1], 24);
        assert_eq!(modified.registers.attribute.values[0x10], 3);
        assert_eq!(modified.registers.dac.colors[24], [0; 3]);

        video(&mut dos, 0x008f, 0, 0, 0);
        let preserved = dos.vga().borrow().snapshot();
        assert_eq!(
            preserved.registers.attribute.values,
            initial.registers.attribute.values
        );
        assert_eq!(preserved.registers.dac.colors, initial.registers.dac.colors);
        for plane in 0..4 {
            assert_eq!(preserved.plane(plane), before.plane(plane));
        }
        assert_eq!(
            video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
            0x508f
        );
        // A later color mode switches both CRTC and attribute-status routing back.
        video(&mut dos, 0x0010, 0, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u16(0x463).unwrap(), 0x3d4);
        let color = dos.vga().borrow().snapshot();
        assert_eq!(color.registers.attribute.values[0x10], 1);
        assert_eq!(color.registers.attribute.values[0x12], 15);
        assert_eq!(color.registers.dac.colors[1], [0, 0, 42]);
    });
}

#[test]
fn planar_modes_restore_routing_clear_all_planes_and_report_geometry() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, size, columns, rows, height, page_bytes, clocking) in [
            (0x0d, [320, 200], 40, 25, 8, 0x2000, 9),
            (0x0e, [640, 200], 80, 25, 8, 0x4000, 1),
            (0x0f, [640, 350], 80, 25, 14, 0x8000, 1),
            (0x10, [640, 350], 80, 25, 14, 0x8000, 1),
            (0x11, [640, 480], 80, 30, 16, 0xa000, 1),
            (0x12, [640, 480], 80, 30, 16, 0xa000, 1),
        ] {
            video(&mut dos, 0x0013, 0, 0, 0);
            {
                let mut context = dos.cpu().context();
                context.io.write_port(0x3c4, 2, 0x0604); // All 64 KiB of each plane.
                context.memory.write(0xa0000, &[0xa5; 65536]).unwrap();
                context.io.write_port(0x3c4, 2, 0x0102); // Plane zero only.
                context.io.write_port(0x3ce, 2, 0x0f01); // Enable set/reset.
                context.io.write_port(0x3ce, 2, 0x0008); // Disable all bit writes.
                context.io.write_port(0x3ce, 2, 0x0305); // Write mode three.
            }
            assert_eq!(video(&mut dos, mode, 0, 0, 0).registers.eax as u16, 0x20);
            let display = dos.vga().borrow().snapshot();
            assert_eq!(display.geometry().size, size);
            for plane in 0..4 {
                assert!(
                    display.plane(plane).iter().all(|&byte| byte == 0),
                    "mode {mode:x}, plane {plane}"
                );
            }
            let result = video(&mut dos, 0x0f00, 0xffa5, 0, 0);
            assert_eq!(result.registers.eax as u16, columns * 256 + mode);
            assert_eq!(result.registers.ebx as u16, 0xa5);
            let mut context = dos.cpu().context();
            assert_eq!(context.memory.read_u16(0x44c).unwrap(), page_bytes);
            assert_eq!(context.memory.read_u8(0x484).unwrap(), rows - 1);
            assert_eq!(context.memory.read_u16(0x485).unwrap(), height);
            let crtc_port = if mode == 0x0f { 0x3b4 } else { 0x3d4 };
            assert_eq!(context.memory.read_u16(0x463).unwrap(), crtc_port);
            let compare_planes = match mode {
                0x0f => 5,
                0x11 => 1,
                _ => 15,
            };
            for (port, index, expected) in [
                (0x3c4, 0, 3),
                (0x3c4, 1, clocking),
                (0x3c4, 2, 15),
                (0x3c4, 4, 6),
                (0x3ce, 1, 0),
                (0x3ce, 5, 0),
                (0x3ce, 6, 5),
                (0x3ce, 7, compare_planes),
                (0x3ce, 8, 255),
                (crtc_port, 0x14, if mode == 0x0f { 15 } else { 0 }),
                (crtc_port, 0x17, if mode == 0x11 { 0xc3 } else { 0xe3 }),
            ] {
                context.io.write_port(port, 1, index);
                assert_eq!(context.io.read_port(port + 1, 1), expected);
            }
            assert_eq!(context.memory.read_u8(0xb8000).unwrap(), 255);
            // The normal CPU aperture writes all planes, and read-map selection
            // can observe every one of them, including memory outside the image.
            context.memory.write_u8(0xaffff, 0x96).unwrap();
            for plane in 0..4 {
                context.io.write_port(0x3ce, 2, (plane << 8) | 4);
                assert_eq!(context.memory.read_u8(0xaffff).unwrap(), 0x96);
            }
        }
    });
}

#[test]
fn planar_no_clear_preserves_each_plane_and_restores_mode_specific_palettes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, columns, palette) in [
            (
                0x0d,
                40,
                [
                    0, 1, 2, 3, 4, 5, 6, 7, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                ],
            ),
            (
                0x0e,
                80,
                [
                    0, 1, 2, 3, 4, 5, 6, 7, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                ],
            ),
            (
                0x10,
                80,
                [
                    0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
                ],
            ),
            (
                0x12,
                80,
                [
                    0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
                ],
            ),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            for plane in 0..4u32 {
                let mut context = dos.cpu().context();
                context.io.write_port(0x3c4, 2, (1 << (plane + 8)) | 2);
                context
                    .memory
                    .write(0xa0000, &vec![0x31 + plane as u8; 65536])
                    .unwrap();
            }
            // No-clear preserves memory but still restores guest-modified colors.
            {
                let context = dos.cpu().context();
                context.io.read_port(0x3da, 1);
                context.io.write_port(0x3c0, 1, 6);
                context.io.write_port(0x3c0, 1, 1);
                context.io.write_port(0x3c0, 1, 0x20);
                context.io.write_port(0x3c8, 1, u32::from(palette[6]));
                for _ in 0..3 {
                    context.io.write_port(0x3c9, 1, 0);
                }
            }
            let before = dos.vga().borrow().snapshot();
            video(&mut dos, mode | 0x80, 0, 0, 0);
            assert_eq!(
                video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
                columns * 256 + mode + 0x80
            );
            let preserved = dos.vga().borrow().snapshot();
            for plane in 0..4 {
                assert_eq!(preserved.plane(plane), before.plane(plane));
            }
            assert_eq!(&preserved.registers.attribute.values[..16], &palette);
            let brown = palette[6] as usize;
            assert_eq!(preserved.registers.dac.colors[brown], [42, 21, 0]);
            assert_eq!(
                preserved.registers.dac.colors[palette[9] as usize],
                [21, 21, 63]
            );
            // These entries differ even though both default palettes show brown.
            assert_eq!(
                preserved.registers.dac.colors[6],
                if matches!(mode, 0x0d | 0x0e) {
                    [42, 21, 0]
                } else {
                    [42, 42, 0]
                }
            );
            assert_eq!(
                preserved.registers.dac.colors[0x14],
                if matches!(mode, 0x0d | 0x0e) {
                    [63, 21, 21]
                } else {
                    [42, 21, 0]
                }
            );
        }
    });
}
