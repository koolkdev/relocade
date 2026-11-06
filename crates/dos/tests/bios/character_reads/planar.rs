use super::read_character;
use crate::{
    common,
    video::{video, PROGRAM},
};
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn planar_reads_use_each_modes_page_stride_and_full_font() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let glyph = [
            0x81, 0x42, 0x24, 0x18, 0xff, 0, 0x80, 1, 0xaa, 0x55, 0xf0, 0x0f, 0x33, 0xcc, 0x5a,
            0xa5,
        ];
        for (mode, stride, height, page, offset) in [
            (0x0d, 40, 8, 7, 0xe000),
            (0x0e, 80, 8, 3, 0xc000),
            (0x0f, 80, 14, 1, 0x8000),
            (0x10, 80, 14, 1, 0x8000),
            (0x11, 80, 16, 0, 0),
            (0x12, 80, 16, 0, 0),
        ] {
            video(&mut dos, mode, 0, 0, 0);
            video(&mut dos, 0x0200, u16::from(page) << 8, 0, 0x0103);
            {
                let mut context = dos.cpu().context();
                context.memory.write_u32(0x10c, 0x2300_0000).unwrap();
                context.memory.write_u32(0x7c, 0).unwrap();
                context.memory.write(0x23000, &[0xff; 4096]).unwrap();
                context
                    .memory
                    .write(0x23000 + 128 * height, &glyph[..height as usize])
                    .unwrap();
                context.io.write_port(0x3c4, 2, 0x0102); // Write plane zero directly.
                for (line, &bits) in glyph[..height as usize].iter().enumerate() {
                    context
                        .memory
                        .write_u8(0xa0000 + offset + (height + line as u32) * stride + 3, bits)
                        .unwrap();
                }
                context.io.write_port(0x3ce, 2, 0x0305); // Restore the caller's write mode too.
                context.io.write_port(0x3ce, 2, 0x0304);
            }
            let before = dos.vga().borrow().snapshot();
            let before_registers = dos.vga().borrow().registers().clone();
            assert_eq!(read_character(&mut dos, page), 128, "mode {mode:02x}");
            {
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
                assert_eq!(
                    context
                        .memory
                        .read_u16(0x450 + u32::from(page) * 2)
                        .unwrap(),
                    0x0103
                );
            }
            let after = dos.vga().borrow().snapshot();
            let after_registers = dos.vga().borrow().registers().clone();
            assert_eq!(after_registers, before_registers);
            for plane in 0..4 {
                assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
            }
        }
    });
}

#[test]
fn planar_reads_honor_color_comparison_and_programmable_font_height() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x12, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x2300));
            context.cpu.set_bp(0);
            context.memory.write(0x23000, &[0xff; 768]).unwrap();
            context
                .memory
                .write(0x23000 + 65 * 3, &[0x81, 0x42, 0x24])
                .unwrap();
            context
                .memory
                .write(0x23000 + 66 * 3, &[0x7e, 0xbd, 0xdb])
                .unwrap();
            context.memory.write(0x23000 + 67 * 3, &[0; 3]).unwrap();
            context.io.write_port(0x3c4, 2, 0x0102);
            for (address, byte) in [(0xa00f2, 0x7e), (0xa0142, 0xbd), (0xa0192, 0xdb)] {
                context.memory.write_u8(address, byte).unwrap();
            }
            // Uncared planes must not contribute to the character bitmap.
            context.io.write_port(0x3c4, 2, 0x0e02);
            for address in [0xa00f2, 0xa0142, 0xa0192] {
                context.memory.write_u8(address, 0xff).unwrap();
            }
        }
        video(&mut dos, 0x1121, 0, 3, 4);
        video(&mut dos, 0x0200, 0, 0, 0x0102);
        for (compare, care, character) in [(1, 1, 65), (0, 1, 66), (0, 0, 67)] {
            {
                let context = dos.cpu().context();
                context.io.write_port(0x3ce, 2, compare << 8 | 2);
                context.io.write_port(0x3ce, 2, care << 8 | 7);
            }
            let before = dos.vga().borrow().snapshot();
            let before_registers = dos.vga().borrow().registers().clone();
            assert_eq!(read_character(&mut dos, 0), character);
            let after = dos.vga().borrow().snapshot();
            let after_registers = dos.vga().borrow().registers().clone();
            assert_eq!(after_registers, before_registers);
            for plane in 0..4 {
                assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
            }
        }
    });
}
