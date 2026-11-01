use super::video::{video, PROGRAM};
use crate::common;
use relocade_dos::Dos;
use relocade_x86::{Segment, SegmentRegister};

fn source(dos: &mut Dos, segment: u16, offset: u16) {
    let context = dos.cpu().context();
    context.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, segment),
    );
    context.cpu.set_bp(offset);
}

#[test]
fn string_formats_wrap_source_offsets_and_optionally_update_the_requested_page_cursor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for format in 0..4 {
            video(&mut dos, 3, 0, 0, 0);
            video(&mut dos, 0x0200, 0x0700, 0, 0x0203);
            source(&mut dos, 0x5000, 0xffff);
            let bytes: &[u8] = if format & 2 == 0 {
                b"ABC"
            } else {
                &[b'A', 0x1e, b'B', 0x2f, b'C', 0x3a]
            };
            {
                let mut context = dos.cpu().context();
                context.memory.write_u8(0x5ffff, bytes[0]).unwrap();
                context.memory.write(0x50000, &bytes[1..]).unwrap();
                context.memory.write(0x60000, &[0xff; 5]).unwrap();
            }
            let before = dos.vga().borrow().snapshot();
            let cpu = video(&mut dos, 0x1300 | format, 0x077c, 3, 0x014e);
            assert_eq!(cpu.registers.eax as u16, 0x1300 | format);
            assert_eq!(cpu.registers.ebx as u16, 0x077c);
            assert_eq!(cpu.registers.ecx as u16, 3);
            assert_eq!(cpu.registers.edx as u16, 0x014e);
            assert_eq!(cpu.registers.ebp as u16, 0xffff);
            assert_eq!(cpu.segments.es.selector, 0x5000);
            let context = dos.cpu().context();
            let mut cells = [0; 6];
            context.memory.read(0xbf13c, &mut cells).unwrap();
            assert_eq!(
                cells,
                if format & 2 == 0 {
                    [b'A', 0x7c, b'B', 0x7c, b'C', 0x7c]
                } else {
                    [b'A', 0x1e, b'B', 0x2f, b'C', 0x3a]
                }
            );
            assert_eq!(
                context.memory.read_u16(0x45e).unwrap(),
                if format & 1 == 0 { 0x0203 } else { 0x0201 }
            );
            assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
            assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0);
            assert_eq!(context.memory.read_u16(0x450).unwrap(), 0);
            assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        }
    });
}

#[test]
fn string_controls_consume_attribute_pairs_and_scroll_only_the_requested_page() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0200, 0x0100, 0, 2);
        source(&mut dos, 0x5000, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u16(0x44a, 3).unwrap();
            context.memory.write_u8(0x484, 1).unwrap(); // A 3-column, 2-row BIOS grid.
            context.memory.write(0xb8000, &[0x55; 12]).unwrap();
            context
                .memory
                .write(0xb9006, &[b'x', 0x4e, b'y', 0x5a, b'z', 0x6b])
                .unwrap();
            context
                .memory
                .write(
                    0x50000,
                    &[
                        b'A', 0x11, 7, 0xaa, b'B', 0x22, 8, 0xbb, b'C', 0x33, b'\n', 0xcc, b'\r',
                        0xdd, b'D', 0x44,
                    ],
                )
                .unwrap();
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1302, 0x017c, 8, 2);
        let context = dos.cpu().context();
        let mut cells = [0; 12];
        context.memory.read(0xb9000, &mut cells).unwrap();
        // LF at column one takes the fill attribute from that position after
        // advance (5Ah), not BL (7Ch), the last glyph (33h) or LF's pair (CCh).
        assert_eq!(
            cells,
            [b'C', 0x33, b'y', 0x5a, b'z', 0x6b, b'D', 0x44, b' ', 0x5a, b' ', 0x5a]
        );
        context.memory.read(0xb8000, &mut cells).unwrap();
        assert_eq!(cells, [0x55; 12]);
        assert_eq!(context.memory.read_u16(0x452).unwrap(), 2);
        assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
        assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
    });
}

#[test]
fn graphics_strings_use_selected_fonts_and_scroll_the_hidden_page_with_black_fill() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0d, 0, 0, 0);
        source(&mut dos, 0x2200, 0);
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write(0x22000 + 65 * 3, &[0x80, 0x40, 0x20])
                .unwrap();
            context
                .memory
                .write(0x22000 + 128 * 3, &[0x81, 0x42, 0x24])
                .unwrap();
        }
        video(&mut dos, 0x1121, 0, 3, 2);
        dos.cpu().context().memory.write_u16(0x44a, 3).unwrap();
        video(&mut dos, 0x0507, 0, 0, 0);
        video(&mut dos, 0x0600, 0x0500, 0, 0xffff);
        video(&mut dos, 0x0500, 0, 0, 0);
        source(&mut dos, 0x5000, 0);
        dos.cpu()
            .context()
            .memory
            .write(0x50000, &[b'A', 6, 0x80, 9])
            .unwrap();
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1303, 0x0700, 2, 0x0102);
        let after = dos.vga().borrow().snapshot();
        for plane in 0..4 {
            let mut expected = before.plane(plane).to_vec();
            for y in 0..3 {
                expected[0xe000 + y * 40 + 2] = if plane == 1 || plane == 2 {
                    0x80 >> y
                } else {
                    0
                };
            }
            for y in 3..6 {
                expected[0xe000 + y * 40..0xe000 + y * 40 + 3].fill(0);
                if plane == 0 || plane == 3 {
                    expected[0xe000 + y * 40] = [0x81, 0x42, 0x24][y - 3];
                }
            }
            assert_eq!(after.plane(plane), expected);
        }
        assert_eq!(after.registers, before.registers);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u16(0x45e).unwrap(), 0x0101);
        assert_eq!(context.memory.read_u16(0x450).unwrap(), 0);
        assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
        assert_eq!(context.memory.read_u16(0x44e).unwrap(), 0);
    });
}

#[test]
fn mode_11_strings_force_white_and_preserve_the_xor_flag() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x11, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x43 * 4, 0x2300_0000).unwrap();
            context
                .memory
                .write(0x23000 + 65 * 16, &[0x81; 16])
                .unwrap();
            context.memory.write_u8(0x50000, b'A').unwrap();
        }
        source(&mut dos, 0x5000, 0);
        video(&mut dos, 0x0200, 0, 0, 0x0203);
        video(&mut dos, 0x1300, 0, 1, 0);
        let white = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1300, 0x80, 1, 0);
        let erased = dos.vga().borrow().snapshot();
        for plane in 0..4 {
            for y in 0..16 {
                assert_eq!(white.plane(plane)[y * 80], 0x81);
                assert_eq!(erased.plane(plane)[y * 80], 0);
            }
        }
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x0203);
    });
}

#[test]
fn string_source_reads_remain_live_when_destination_overwrites_later_pairs() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        source(&mut dos, 0xb800, 0);
        dos.cpu()
            .context()
            .memory
            .write(0xb8000, &[b'A', 0x1e, b'B', 0x2f, b'C', 0x3a])
            .unwrap();
        video(&mut dos, 0x1303, 0, 3, 1);
        let mut cells = [0; 8];
        dos.cpu()
            .context()
            .memory
            .read(0xb8000, &mut cells)
            .unwrap();
        assert_eq!(cells, [b'A', 0x1e, b'A', 0x1e, b'A', 0x1e, b'A', 0x1e]);
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 4);
        assert_eq!(common::screen(&dos).cursor.address, 4);
    });
}

#[test]
fn empty_strings_leave_cursors_untouched_and_invalid_requests_have_no_output() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        source(&mut dos, 0xffff, 0xffff);
        video(&mut dos, 0x0200, 0, 0, 0x0203);
        let before = dos.vga().borrow().snapshot();
        assert_eq!(
            video(&mut dos, 0x1303, 0xff7c, 0, 0xffff).registers.eax as u16,
            0x1303
        );
        assert_eq!(
            video(&mut dos, 0x1304, 0, 1, 0).registers.eax as u16,
            0x8604
        );
        assert_eq!(
            video(&mut dos, 0x1300, 0x0800, 1, 0).registers.eax as u16,
            0x8600
        );
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            assert_eq!(after.plane(plane), before.plane(plane));
        }
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x0203);
    });
}
