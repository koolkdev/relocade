use super::read_character;
use crate::{
    common,
    video::{video, PROGRAM},
};
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn indexed_reads_use_raw_color_indices_and_programmable_font_heights() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for height in [1u16, 3, 32] {
            video(&mut dos, 0x13, 0, 0, 0);
            {
                let mut context = dos.cpu().context();
                context
                    .cpu
                    .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x2300));
                context.cpu.set_bp(0);
                context.memory.write_u32(0x7c, 0).unwrap();
                context.memory.write(0x23000, &[0xff; 8192]).unwrap();
                let mut glyph = [0x81; 32];
                glyph[usize::from(height) - 1] = 0x42;
                context
                    .memory
                    .write(
                        0x23000 + 128 * u32::from(height),
                        &glyph[..usize::from(height)],
                    )
                    .unwrap();
                for line in 0..height {
                    let pixels = if line + 1 == height {
                        [0, 1, 0, 0, 0, 0, 0x40, 0]
                    } else {
                        [0x80, 0, 0, 0, 0, 0, 0, 0xff]
                    };
                    context
                        .memory
                        .write(0xa0000 + u32::from(height + line) * 320 + 16, &pixels)
                        .unwrap();
                }
                // All displayed colors become black without changing the raw indices.
                context.io.write_port(0x3c6, 1, 0);
                context.io.write_port(0x3c8, 1, 0);
                for _ in 0..3 {
                    context.io.write_port(0x3c9, 1, 0);
                }
            }
            video(&mut dos, 0x1121, 0, height, 4);
            video(&mut dos, 0x0200, 0, 0, 0x0102);
            let before = dos.vga().borrow().snapshot();
            let before_registers = dos.vga().borrow().registers().clone();
            assert_eq!(read_character(&mut dos, 0), 128, "height {height}");
            {
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u16(0x450).unwrap(), 0x0102);
                assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
            }
            let after = dos.vga().borrow().snapshot();
            let after_registers = dos.vga().borrow().registers().clone();
            assert_eq!(after_registers, before_registers);
            for plane in 0..4 {
                assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
            }
        }
        video(&mut dos, 0x0200, 0, 0, 0x0128); // Column 40 is outside mode 13h's grid.
        assert_eq!(read_character(&mut dos, 0), 0);
        video(&mut dos, 0x0200, 0, 0, 0x0102);
        // Corrupt BIOS heights must not overrun the fixed glyph buffer.
        for height in [0, 33] {
            dos.cpu().context().memory.write_u16(0x485, height).unwrap();
            assert_eq!(read_character(&mut dos, 0), 0);
        }
    });
}
