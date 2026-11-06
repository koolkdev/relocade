use super::{read_character, GLYPH};
use crate::{
    common,
    video::{video, PROGRAM},
};
use relocade_bios::video::{self as bios, CharacterCell};

#[test]
fn packed_reads_recognize_direct_pixels_and_preserve_display_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for mode in [4, 5, 6] {
            video(&mut dos, mode, 0, 0, 0);
            video(&mut dos, 0x0200, 0, 0, 0x0203);
            {
                let mut context = dos.cpu().context();
                context.memory.write_u32(0x10c, 0x2300_0000).unwrap();
                context.memory.write(0x23000, &[0xff; 1024]).unwrap();
                context.memory.write(0x23000 + 65 * 8, &GLYPH).unwrap();
                context.memory.write_u16(0x485, 13).unwrap(); // Packed cells remain eight high.

                // Three nonzero colors produce the same bitmap; these bytes
                // cover both CGA scanline banks at row 2, column 3.
                let packed = [
                    [0x40, 0x03],
                    [0x20, 0x04],
                    [0x0c, 0x20],
                    [0x01, 0xc0],
                    [0x6d, 0xb6],
                    [0x00, 0x00],
                    [0xc0, 0x00],
                    [0x00, 0x02],
                ];
                for (line, offsets) in [0x280, 0x2280, 0x2d0, 0x22d0, 0x320, 0x2320, 0x370, 0x2370]
                    .into_iter()
                    .enumerate()
                {
                    if mode == 6 {
                        context
                            .memory
                            .write_u8(0xb8000 + offsets + 3, GLYPH[line])
                            .unwrap();
                    } else {
                        context
                            .memory
                            .write(0xb8000 + offsets + 6, &packed[line])
                            .unwrap();
                    }
                }
                // Incoming compare reads and register indices must be restored.
                context
                    .io
                    .write_port(0x3ce, 2, if mode == 6 { 0x0805 } else { 0x3805 });
                context.io.write_port(0x3ce, 2, 0x0304);
                context.io.write_port(0x3c4, 2, 0x0502);
            }
            let before = dos.vga().borrow().snapshot();
            let before_registers = dos.vga().borrow().registers().clone();
            assert_eq!(read_character(&mut dos, 0), 65);
            {
                let context = dos.cpu().context();
                assert_eq!(
                    bios::read_character(&context.memory, context.io, 0),
                    Some(CharacterCell {
                        character: 65,
                        attribute: 0
                    })
                );
                assert_eq!(context.memory.read_u16(0x450).unwrap(), 0x0203);
                assert_eq!(context.memory.read_u8(0x462).unwrap(), 0);
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
fn packed_reads_search_live_split_fonts_in_order_with_wrapping_and_null_vectors() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 6, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x10c, 0x2300_0000).unwrap();
            context.memory.write(0x23000, &[0xff; 2048]).unwrap();
            // The lower font's second half is not searched in packed modes.
            context.memory.write(0x23000 + 128 * 8, &GLYPH).unwrap();
            context.memory.write_u32(0x7c, 0x2500_fffc).unwrap();
            context.memory.write(0x34ffc, &GLYPH[..4]).unwrap();
            context.memory.write(0x25000, &GLYPH[4..]).unwrap();
            for (line, &bits) in GLYPH.iter().enumerate() {
                let offset = (line & 1) * 0x2000 + (line / 2) * 80;
                context
                    .memory
                    .write_u8(0xb8000 + offset as u32, bits)
                    .unwrap();
            }
        }
        assert_eq!(read_character(&mut dos, 0), 128);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x7c, 0).unwrap();
            context.memory.write(0, &GLYPH).unwrap(); // A null vector must not search the IVT.
        }
        assert_eq!(read_character(&mut dos, 0), 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x7c, 0x2500_fffc).unwrap();
            context.memory.write(0x23000 + 7 * 8, &GLYPH).unwrap();
            context.memory.write(0x23000 + 11 * 8, &GLYPH).unwrap();
        }
        assert_eq!(read_character(&mut dos, 0), 7); // Lowest exact match wins.
        dos.cpu()
            .context()
            .memory
            .write(0x23000 + 7 * 8, &[0xff; 8])
            .unwrap();
        assert_eq!(read_character(&mut dos, 0), 11); // Font changes are observed immediately.
        dos.cpu().context().memory.write_u8(0xba0f0, 2).unwrap(); // Change only the last row.
        assert_eq!(read_character(&mut dos, 0), 0); // Every glyph row must match.
    });
}

#[test]
fn packed_read_restores_access_before_matching_a_font_in_vram() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 6, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            for (line, &bits) in GLYPH.iter().enumerate() {
                let offset = (line & 1) * 0x2000 + (line / 2) * 80;
                context
                    .memory
                    .write_u8(0xb8000 + offset as u32, bits)
                    .unwrap();
            }
            context.memory.write_u32(0x10c, 0xb800_1000).unwrap();
            context.io.write_port(0x3c4, 2, 0x0402); // Font in plane two, pixels in plane zero.
            context.memory.write(0xb9000, &[0xff; 1024]).unwrap();
            context.memory.write(0xb9000 + 65 * 8, &GLYPH).unwrap();
            context.io.write_port(0x3ce, 2, 0x0204); // Caller's font-read plane.
        }
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers().clone();
        assert_eq!(read_character(&mut dos, 0), 65);
        let after = dos.vga().borrow().snapshot();
        let after_registers = dos.vga().borrow().registers().clone();
        assert_eq!(after_registers, before_registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
    });
}

#[test]
fn packed_blank_cells_use_the_font_and_invalid_pages_leave_state_alone() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 4, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x10c, 0x2300_0000).unwrap();
            context.memory.write(0x23000, &[0xff; 1024]).unwrap();
            context.memory.write(0x23000 + 5 * 8, &[0; 8]).unwrap();
            context.memory.write(0x23000 + 32 * 8, &[0; 8]).unwrap();
        }
        assert_eq!(read_character(&mut dos, 0), 5); // Blank does not imply ASCII space.
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers().clone();
        assert_eq!(read_character(&mut dos, 1), 0x86a5);
        {
            let mut context = dos.cpu().context();
            assert_eq!(bios::read_character(&context.memory, context.io, 1), None);
            context.memory.write_u16(0x44a, 2).unwrap();
            context.memory.write_u16(0x450, 2).unwrap(); // Outside the declared BIOS grid.
        }
        assert_eq!(read_character(&mut dos, 0), 0);
        let after = dos.vga().borrow().snapshot();
        let after_registers = dos.vga().borrow().registers().clone();
        assert_eq!(after_registers, before_registers);
        for plane in 0..4 {
            assert!(std::ptr::eq(after.plane(plane), before.plane(plane)));
        }
    });
}
