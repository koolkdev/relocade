use super::{common, video, PROGRAM};
use relocade_x86::{Segment, SegmentRegister};

#[test]
fn graphics_font_selection_changes_bios_metadata_without_uploading_fonts() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x13, 0, 0, 0);
        let before = dos.vga().borrow().snapshot();
        let mut mode_data = [0; 30];
        dos.cpu()
            .context()
            .memory
            .read(0x449, &mut mode_data)
            .unwrap();
        let high_font = dos.cpu().context().memory.read_u32(0x7c).unwrap();
        for (function, rom, height, selector, rows) in [
            (0x1122, 2, 14, 1, 14),
            (0x1123, 3, 8, 3, 43),
            (0x1124, 6, 16, 2, 25),
            (0x1124, 6, 16, 0, 30),
            (0x1124, 6, 16, 0xff, 25),
            (0x1124, 6, 16, 0, 0),
        ] {
            let font = video(&mut dos, 0x1130, rom << 8, 0, 0);
            let pointer =
                (u32::from(font.segments.es.selector) << 16) | (font.registers.ebp & 0xffff);
            {
                let context = dos.cpu().context();
                context
                    .cpu
                    .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x2000));
                context.cpu.set_bp(0xfffb);
            }
            let input_rows = if selector == 0 { rows } else { 0xa5 };
            let result = video(&mut dos, function, 0xa500 | selector, 0x1234, input_rows);
            assert_eq!(result.registers.eax as u16, function);
            assert_eq!(result.registers.ebx as u16, 0xa500 | selector);
            assert_eq!(result.registers.ecx as u16, 0x1234);
            assert_eq!(result.registers.edx as u16, input_rows);
            assert_eq!(result.segments.es.selector, 0x2000);
            assert_eq!(result.registers.ebp as u16, 0xfffb);
            let info = video(&mut dos, 0x1130, 0x0100, 0, 0xa500);
            assert_eq!(info.segments.es.selector, (pointer >> 16) as u16);
            assert_eq!(info.registers.ebp as u16, pointer as u16);
            assert_eq!(info.registers.ecx as u16, height);
            assert_eq!(info.registers.edx as u8, (rows as u8).wrapping_sub(1));
        }
        let context = dos.cpu().context();
        let mut actual = [0; 30];
        context.memory.read(0x449, &mut actual).unwrap();
        assert_eq!(actual, mode_data); // Pages, columns and cursors are unchanged.
        assert_eq!(context.memory.read_u32(0x7c).unwrap(), high_font);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
        // Geometry-changing graphics font calls are also metadata-only in text.
        video(&mut dos, 3, 0, 0, 0);
        let text = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1122, 2, 0, 0);
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 14);
        let selected = dos.vga().borrow().snapshot();
        assert_eq!(selected.registers, text.registers);
        assert!(std::ptr::eq(selected.plane(2), text.plane(2)));
    });
}

#[test]
fn graphics_font_vectors_are_live_and_user_glyphs_wrap_the_segment_offset() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        // INT 1Fh can be set while text is active and survives the next mode set.
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x2400));
            context.cpu.set_bp(0);
            context.memory.write(0x24000, &[0xff; 8]).unwrap();
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x1120, 0xffff, 0, 0xffff); // Geometry arguments are ignored.
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.plane(2), before.plane(2));
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 8);
        video(&mut dos, 0x0d, 0, 0, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u32(0x7c).unwrap(),
            0x2400_0000
        );
        let glyph = [0x80, 0x40, 0x20, 0x10, 8, 4, 2, 1];
        {
            let mut context = dos.cpu().context();
            context
                .cpu
                .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x2200));
            context.cpu.set_bp(0xfbfc); // Character 80h starts at 2200:FFFC.
            context.memory.write(0x31ffc, &glyph[..4]).unwrap();
            context.memory.write(0x22000, &glyph[4..]).unwrap();
        }
        video(&mut dos, 0x1121, 0, 8, 25);
        video(&mut dos, 0x0200, 0, 0, 0x0102);
        video(&mut dos, 0x0980, 4, 1, 0);
        let first = dos.vga().borrow().snapshot();
        for (y, &row) in glyph.iter().enumerate() {
            assert_eq!(first.plane(2)[(8 + y) * 40 + 2], row);
        }
        dos.cpu().context().memory.write_u8(0x31ffc, 0x81).unwrap();
        video(&mut dos, 0x0200, 0, 0, 0x0103);
        video(&mut dos, 0x0980, 4, 1, 0);
        assert_eq!(dos.vga().borrow().snapshot().plane(2)[8 * 40 + 3], 0x81);
        // A custom height changes placement as well as the returned BIOS fields.
        {
            let mut context = dos.cpu().context();
            context.cpu.set_bp(0);
            context
                .memory
                .write(0x22000 + 65 * 3, &[0x81, 0x42, 0x24])
                .unwrap();
        }
        video(&mut dos, 0x1121, 0, 3, 4);
        video(&mut dos, 0x0200, 0, 0, 0x0204);
        video(&mut dos, 0x0941, 1, 1, 0);
        let custom = dos.vga().borrow().snapshot();
        for (y, expected) in [(5, 0), (6, 0x81), (7, 0x42), (8, 0x24), (9, 0)] {
            assert_eq!(custom.plane(0)[y * 40 + 4], expected);
        }
        let info = video(&mut dos, 0x1130, 0x0100, 0, 0);
        assert_eq!(info.registers.ecx as u16, 3);
        assert_eq!(info.registers.edx as u8, 3);
    });
}
