use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn cga_modes_restore_b8000_routing_and_honor_memory_preservation() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (mode, width, control, color, result) in [
            (4, 320, 0x2a, 0x30, 0x30),
            (5, 320, 0x2e, 0x30, 0x30),
            (6, 640, 0x1e, 0x3f, 0x3f),
        ] {
            // Leave a nonzero byte in every plane so clearing unused planes is observable.
            video(&mut dos, 0x0d, 0, 0, 0);
            dos.cpu().context().memory.write_u8(0xa0000, 0x5a).unwrap();
            let seeded = dos.vga().borrow().snapshot();
            assert_eq!(video(&mut dos, mode, 0, 0, 0).registers.eax as u16, result);
            dos.cpu()
                .context()
                .memory
                .write(0xb8000, &[0xa5; 32768])
                .unwrap();
            let retained = dos.vga().borrow().snapshot();
            {
                let context = dos.cpu().context();
                for (port, word) in [
                    (0x3c4, 0x0002),
                    (0x3c4, 0x0e04),
                    (0x3ce, 0x0f01),
                    (0x3ce, 0x1b03),
                    (0x3ce, 0x4b05),
                    (0x3ce, 0x0506),
                    (0x3ce, 0x0008),
                ] {
                    context.io.write_port(port, 2, word);
                }
            }
            video(&mut dos, 0x80 | mode, 0, 0, 0);
            let preserved = dos.vga().borrow().snapshot();
            for plane in 0..4 {
                assert_eq!(preserved.plane(plane), retained.plane(plane));
            }
            assert_eq!(preserved.geometry().size, [width, 200]);
            assert_eq!(preserved.geometry().scanline_step, 2);
            assert_eq!(
                video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u16,
                ((width as u16 / 8) << 8) | mode | 0x80
            );
            {
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x4000);
                assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), 8);
                assert_eq!(context.memory.read_u8(0x465).unwrap(), control);
                assert_eq!(context.memory.read_u8(0x466).unwrap(), color);
                assert_eq!(context.memory.read_u8(0xa0000).unwrap(), 255);
            }
            assert_eq!(
                video(&mut dos, 0x0501, 0, 0, 0).registers.eax as u16,
                0x8601
            );
            video(&mut dos, mode, 0, 0, 0);
            let mut clear = vec![255; 32768];
            dos.cpu()
                .context()
                .memory
                .read(0xb8000, &mut clear)
                .unwrap();
            assert!(clear.iter().all(|&byte| byte == 0));
            for plane in (if mode == 6 { 1 } else { 2 })..4 {
                assert_eq!(
                    dos.vga().borrow().snapshot().plane(plane),
                    seeded.plane(plane)
                );
            }
        }
    });
}

#[test]
fn packed_pixels_preserve_neighbors_banks_and_guest_access_registers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [4, 6] {
            video(&mut dos, mode, 0, 0, 0);
            dos.cpu()
                .context()
                .memory
                .write(0xb8000, &[0x55; 32768])
                .unwrap();
            {
                let context = dos.cpu().context();
                for (port, word) in [
                    (0x3c4, 0x0002),
                    (0x3ce, 0x0200),
                    (0x3ce, 0x0f01),
                    (0x3ce, 0x1b03),
                    (0x3ce, 0x0304),
                    (0x3ce, if mode == 4 { 0x3b05 } else { 0x0b05 }),
                    (0x3ce, 0x5a08),
                ] {
                    context.io.write_port(port, 2, word);
                }
            }
            let before = dos.vga().borrow().snapshot();
            let writes = if mode == 4 {
                [
                    (0, 0, 3),
                    (3, 0, 2),
                    (4, 0, 0),
                    (0, 1, 2),
                    (0, 2, 3),
                    (319, 199, 2),
                ]
            } else {
                [
                    (0, 0, 1),
                    (7, 0, 0),
                    (8, 0, 1),
                    (0, 1, 1),
                    (0, 2, 1),
                    (639, 199, 0),
                ]
            };
            for (x, y, color) in writes {
                video(&mut dos, 0x0c00 | color, 0xff00, x, y);
                assert_eq!(
                    video(&mut dos, 0x0d00, 0xab00, x, y).registers.eax as u8,
                    color as u8
                );
            }
            let (x, xor, toggled, original) = if mode == 4 {
                (3, 0x83, 1, 2)
            } else {
                (7, 0x81, 1, 0)
            };
            for expected in [toggled, original] {
                video(&mut dos, 0x0c00 | xor, 0, x, 0);
                assert_eq!(
                    video(&mut dos, 0x0d00, 0, x, 0).registers.eax as u8,
                    expected
                );
            }
            let after = dos.vga().borrow().snapshot();
            assert_eq!(after.registers, before.registers);
            let bytes = if mode == 4 {
                [
                    (0, 0xd6),
                    (1, 0x15),
                    (0x2000, 0x95),
                    (80, 0xd5),
                    (0x3f3f, 0x56),
                ]
            } else {
                [
                    (0, 0xd4),
                    (1, 0xd5),
                    (0x2000, 0xd5),
                    (80, 0xd5),
                    (0x3f3f, 0x54),
                ]
            };
            for plane in 0..4 {
                let mut expected = before.plane(plane).to_vec();
                for (offset, byte) in bytes {
                    if mode == 4 && plane == (offset & 1) {
                        expected[offset & !1] = byte;
                    } else if mode == 6 && plane == 0 {
                        expected[offset] = byte;
                    }
                }
                assert_eq!(after.plane(plane), expected);
            }
        }
    });
}

#[test]
fn packed_glyphs_scroll_as_bytes_and_mode_six_ignores_foreground_color() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let glyph = [0x81, 0x42, 0x24, 0x18, 0xff, 0, 0x80, 1];
        let pairs = [
            [0x80, 2],
            [0x20, 8],
            [8, 0x20],
            [2, 0x80],
            [0xaa, 0xaa],
            [0, 0],
            [0x80, 0],
            [0, 2],
        ];
        // Literal offsets for the first eight scanlines alternate 8-KiB banks.
        let rows = [0, 0x2000, 80, 0x2050, 160, 0x20a0, 240, 0x20f0];
        for mode in [4, 6] {
            video(&mut dos, mode, 0, 0, 0);
            {
                let mut context = dos.cpu().context();
                context.memory.write_u32(0x1f * 4, 0x2400_0000).unwrap();
                context.memory.write(0x24000, &glyph).unwrap();
                context.memory.write(0xb8000, &[0xa5; 32768]).unwrap();
            }
            video(&mut dos, 0x0200, 0, 0, 1);
            let color = if mode == 4 { 2 } else { 0 };
            let cell_bytes = if mode == 4 { 2 } else { 1 };
            let mut actual = vec![0; 32768];
            for xor in [true, false] {
                video(&mut dos, 0x0a80, color | if xor { 0x80 } else { 0 }, 1, 0);
                let mut expected = vec![0xa5; 32768];
                for (y, &offset) in rows.iter().enumerate() {
                    let bytes = if mode == 4 {
                        &pairs[y][..]
                    } else {
                        &glyph[y..y + 1]
                    };
                    for (index, &byte) in bytes.iter().enumerate() {
                        expected[offset + cell_bytes + index] =
                            if xor { 0xa5 ^ byte } else { byte };
                    }
                }
                dos.cpu()
                    .context()
                    .memory
                    .read(0xb8000, &mut actual)
                    .unwrap();
                assert_eq!(actual, expected);
            }
            assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 1);
            video(&mut dos, 0x0701, 0x1b00, 0x0001, 0x0102);
            let mut expected = vec![0xa5; 32768];
            for (y, &offset) in rows.iter().enumerate() {
                // Fill with the raw BH byte rather than expanding it as a color.
                expected[offset + cell_bytes..offset + cell_bytes * 3].fill(0x1b);
                let bytes = if mode == 4 {
                    &pairs[y][..]
                } else {
                    &glyph[y..y + 1]
                };
                expected[offset + 320 + cell_bytes..offset + 320 + cell_bytes * 2]
                    .copy_from_slice(bytes);
            }
            dos.cpu()
                .context()
                .memory
                .read(0xb8000, &mut actual)
                .unwrap();
            assert_eq!(actual, expected);
            // A final character wraps and scrolls the full page through the same path.
            dos.cpu()
                .context()
                .memory
                .write(0xb8000, &[0; 32768])
                .unwrap();
            let column = if mode == 4 { 39 } else { 79 };
            video(&mut dos, 0x0200, 0, 0, 0x1800 | column);
            video(&mut dos, 0x0e80, color, 0, 0);
            expected.fill(0);
            for (y, &offset) in rows.iter().enumerate() {
                let bytes = if mode == 4 {
                    &pairs[y][..]
                } else {
                    &glyph[y..y + 1]
                };
                // Character row 23 starts at scanline 184.
                let start = offset + 7360 + column as usize * cell_bytes;
                expected[start..start + cell_bytes].copy_from_slice(bytes);
            }
            dos.cpu()
                .context()
                .memory
                .read(0xb8000, &mut actual)
                .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x1800);
        }
    });
}
