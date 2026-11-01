use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn graphics_pages_use_plane_offsets_and_keep_cursors_in_bios_memory() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x000d, 0, 0, 0);
        let before = dos.vga().borrow().snapshot();
        for page in 0..8 {
            video(&mut dos, 0x0200, page << 8, 0, 0x0100 + page);
            assert_eq!(
                video(&mut dos, 0x0300, page << 8, 0, 0).registers.edx as u16,
                0x0100 + page
            );
        }
        for page in [1, 7] {
            video(&mut dos, 0x0500 | page, 0, 0, 0);
            let context = dos.cpu().context();
            assert_eq!(context.memory.read_u8(0x462).unwrap(), page as u8);
            assert_eq!(context.memory.read_u16(0x44e).unwrap(), page * 0x2000);
            let registers = dos.vga().borrow().snapshot().registers.crtc.values;
            assert_eq!(
                [registers[12], registers[13]],
                (page * 0x2000).to_be_bytes()
            );
            assert_eq!(&registers[14..16], &before.registers.crtc.values[14..16]);
        }
        for (mode, invalid_page) in [
            (0x0d, 8),
            (0x0e, 4),
            (0x10, 2),
            (0x11, 1),
            (0x12, 1),
            (0x13, 1),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            let before = dos.vga().borrow().snapshot();
            for ax in [0x0200, 0x0300, 0x0500 | invalid_page] {
                assert_eq!(
                    video(&mut dos, ax, invalid_page << 8, 0, 0x1234)
                        .registers
                        .eax as u16,
                    0x8600 | (ax & 255)
                );
            }
            assert_eq!(dos.cpu().context().memory.read_u8(0x462).unwrap(), 0);
            assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        }
    });
}

#[test]
fn planar_scrolls_copy_each_latched_byte_and_fill_without_xor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, page, offset, stride, height, rows) in [
            (0x0d, 7, 0xe000, 40, 8, 25),
            (0x0e, 3, 0xc000, 80, 8, 25),
            (0x10, 1, 0x8000, 80, 14, 25),
            (0x11, 0, 0, 80, 16, 30),
            (0x12, 0, 0, 80, 16, 30),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            video(&mut dos, 0x0500 | page, 0, 0, 0);
            assert_eq!(
                dos.cpu().context().memory.read_u16(0x44e).unwrap(),
                offset as u16
            );
            let crtc = dos.vga().borrow().snapshot().registers.crtc.values;
            assert_eq!([crtc[12], crtc[13]], (offset as u16).to_be_bytes());
            {
                let mut context = dos.cpu().context();
                for plane in 0..4 {
                    context.io.write_port(0x3c4, 2, (1 << (plane + 8)) | 2);
                    for y in 0..height * 3 {
                        // Both bytes and planes differ, exposing stale latch copies.
                        let byte = (plane * 48 + y) as u8;
                        context
                            .memory
                            .write(
                                0xa0000 + (offset + y * stride) as u32,
                                &[0xa5, byte, !byte, 0x5a],
                            )
                            .unwrap();
                    }
                }
                for (port, value) in [
                    (0x3c4, 0x0502),
                    (0x3ce, 0x1b03),
                    (0x3ce, 0x0304),
                    (0x3ce, 0x0b05),
                    (0x3ce, 0x5a08),
                    (0x3ce, 0x0f01),
                ] {
                    context.io.write_port(port, 2, value);
                }
            }
            let before = dos.vga().borrow().snapshot();
            video(&mut dos, 0x0601, 0x8500, 0x0001, 0x0202);
            let up = dos.vga().borrow().snapshot();
            assert_eq!(up.registers, before.registers);
            video(&mut dos, 0x0701, 0x8900, 0x0001, 0x0202);
            let down = dos.vga().borrow().snapshot();
            assert_eq!(down.registers, before.registers);
            for plane in 0..4 {
                let mut expected_up = before.plane(plane).to_vec();
                let mut expected_down = expected_up.clone();
                for y in 0..height * 3 {
                    let address = offset + y * stride + 1;
                    let source = offset + (y + height) * stride + 1;
                    let up_bytes = if y < height * 2 {
                        [before.plane(plane)[source], before.plane(plane)[source + 1]]
                    } else {
                        [if 5 & (1 << plane) != 0 { 255 } else { 0 }; 2]
                    };
                    expected_up[address..address + 2].copy_from_slice(&up_bytes);
                    let down_bytes = if y < height {
                        [if 9 & (1 << plane) != 0 { 255 } else { 0 }; 2]
                    } else {
                        [
                            before.plane(plane)[address],
                            before.plane(plane)[address + 1],
                        ]
                    };
                    expected_down[address..address + 2].copy_from_slice(&down_bytes);
                }
                assert_eq!(up.plane(plane), expected_up);
                assert_eq!(down.plane(plane), expected_down);
            }
            // Oversized windows clip to the last cell; zero lines means clear.
            let last_cell = ((rows - 1) << 8) | (stride as u16 - 1);
            video(&mut dos, 0x0600, 0x8f00, last_cell, 0xffff);
            let clipped = dos.vga().borrow().snapshot();
            for plane in 0..4 {
                let mut expected = down.plane(plane).to_vec();
                for y in (rows as usize - 1) * height..rows as usize * height {
                    expected[offset + y * stride + stride - 1] = 255;
                }
                assert_eq!(clipped.plane(plane), expected); // Includes other pages and padding.
            }
        }
    });
}

#[test]
fn indexed_scrolls_move_character_rows_and_keep_all_eight_color_bits() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0013, 0, 0, 0);
        let mut initial = vec![0x5a; 65536];
        for y in 0..24 {
            for x in 8..24 {
                initial[y * 320 + x] = (y * 13 + x) as u8;
            }
        }
        dos.cpu().context().memory.write(0xa0000, &initial).unwrap();
        video(&mut dos, 0x0701, 0x8500, 0x0001, 0x0202);
        let mut expected = initial.clone();
        for y in 0..24 {
            let row = &mut expected[y * 320 + 8..y * 320 + 24];
            if y < 8 {
                row.fill(0x85);
            } else {
                row.copy_from_slice(&initial[(y - 8) * 320 + 8..(y - 8) * 320 + 24]);
            }
        }
        let mut actual = vec![0; 65536];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        video(&mut dos, 0x0601, 0xff00, 0x0001, 0x0202);
        for y in 0..24 {
            let row = &mut expected[y * 320 + 8..y * 320 + 24];
            if y < 16 {
                row.copy_from_slice(&initial[y * 320 + 8..y * 320 + 24]);
            } else {
                row.fill(255);
            }
        }
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        // An empty rectangle does not touch pixels or access registers.
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x06ff, 0, 0x0202, 0x0101);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
    });
}
