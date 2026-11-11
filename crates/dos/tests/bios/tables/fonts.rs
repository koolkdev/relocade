use super::{
    common, information, install, video, FarPointer, PARAMETERS, PRIMARY, PROGRAM, SECONDARY,
};

#[test]
fn text_overrides_apply_ranges_geometry_and_the_matching_alternate_bank() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        let glyph = [0x80, 0x40, 0x20, 0x10, 8, 4, 2, 1, 0x55, 0xaa];
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write_wrapping(PRIMARY.wrapping_add(8), &0x5000_0400u32.to_le_bytes())
                .unwrap();
            context
                .memory
                .write(
                    0x50400,
                    &[10, 5, 1, 0, 65, 0, 0xfb, 0xff, 0, 0x70, 18, 3, 0xff],
                )
                .unwrap();
            context
                .memory
                .write_wrapping(FarPointer::from_packed(0x7000_fffb), &glyph)
                .unwrap();
            context
                .memory
                .write_wrapping(SECONDARY.wrapping_add(6), &0x5000_0440u32.to_le_bytes())
                .unwrap();
            // Initially the alternate font's height does not match the primary.
            context
                .memory
                .write(0x50440, &[8, 6, 0, 0, 0x10, 0, 0x80, 3, 0xff])
                .unwrap();
            let alternate: Vec<u8> = (0..2560).map(|offset| (offset / 10) as u8).collect();
            context.memory.write(0x81000, &alternate).unwrap();
            context
                .memory
                .write_wrapping(PARAMETERS.wrapping_add(24 * 64 + 7), &[0x13])
                .unwrap(); // Normal B=7.
        }
        video(&mut dos, 3, 0, 0, 0);
        let display = dos.vga().borrow().snapshot();
        assert_eq!(&display.plane(2)[0x6820..0x682a], &glyph);
        assert_eq!(&display.plane(2)[0x682a..0x6840], &[0; 22]);
        assert_eq!(display.vga().unwrap().sequencer[3], 0x13); // Loading bank 5 did not select it.
        assert_eq!(&display.plane(2)[0xa820..0xa82a], &[0; 10]);
        assert_eq!(display.vga().unwrap().crtc.values[9], 0x49);
        {
            let mut context = dos.cpu().context();
            assert_eq!(context.memory.read_u16(0x485).unwrap(), 10);
            assert_eq!(context.memory.read_u8(0x484).unwrap(), 17);
            assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x2000); // Computed before explicit rows.
            assert_eq!(context.memory.read_u16(0x460).unwrap(), 0x0809);
            context.memory.write_u8(0x50440, 10).unwrap();
            context.memory.write_u8(0x5040a, 0xff).unwrap(); // Keep calculated rows next time.
        }
        video(&mut dos, 3, 0, 0, 0);
        let display = dos.vga().borrow().snapshot();
        assert_eq!(display.vga().unwrap().sequencer[3], 0x3b); // Alternate A=6, normal B=7.
        assert_eq!(&display.plane(2)[0xa820..0xa82a], &[65; 10]);
        assert_eq!(&display.plane(2)[0xbfe0..0xbfea], &[255; 10]);
        assert_eq!(dos.cpu().context().memory.read_u8(0x484).unwrap(), 39);
        assert_eq!(information(&mut dos, 0x4000, 0x200, 0)[0x32], 5);
        video(&mut dos, 1, 0, 0, 0); // Neither mode list includes mode 1.
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 16);
        assert_eq!(dos.vga().borrow().snapshot().vga().unwrap().sequencer[3], 0);
        assert_eq!(information(&mut dos, 0x4000, 0x200, 0)[0x32], 5); // Pointers remain installed.
    });
}

#[test]
fn graphics_override_changes_the_live_font_vector_without_uploading_font_memory() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x13, 0, 0, 0);
        install(&mut dos);
        {
            let mut context = dos.cpu().context();
            // Use a different segment for the header so it cannot overwrite the root table.
            context
                .memory
                .write_wrapping(PRIMARY.wrapping_add(12), &0x8000_fffcu32.to_le_bytes())
                .unwrap();
            context
                .memory
                .write_wrapping(
                    FarPointer::from_packed(0x8000_fffc),
                    &[16, 12, 0, 0, 0x10, 0, 0x70, 0x13, 0xff],
                )
                .unwrap();
            context
                .memory
                .write(0x71000 + 65 * 12, &[0x80; 12])
                .unwrap();
        }
        let before = dos.vga().borrow().snapshot();
        let before_registers = dos.vga().borrow().registers();
        video(&mut dos, 0x93, 0, 0, 0);
        let after = dos.vga().borrow().snapshot();
        let after_registers = dos.vga().borrow().registers();
        assert_eq!(after_registers, before_registers);
        for plane in 0..4 {
            assert_eq!(after.plane(plane), before.plane(plane));
        }
        assert_eq!(
            dos.cpu().context().memory.read_u32(0x10c).unwrap(),
            0x7000_1000
        );
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 12);
        assert_eq!(dos.cpu().context().memory.read_u8(0x484).unwrap(), 15);
        video(&mut dos, 0x0200, 0, 0, 0x0203);
        video(&mut dos, 0x0941, 0xe7, 1, 0);
        assert_eq!(
            dos.cpu()
                .context()
                .memory
                .read_u8(0xa0000 + 24 * 320 + 24)
                .unwrap(),
            0xe7
        );
        assert_eq!(
            dos.cpu()
                .context()
                .memory
                .read_u8(0xa0000 + 24 * 320 + 25)
                .unwrap(),
            0
        );
        assert_eq!(information(&mut dos, 0x4000, 0x200, 0)[0x32], 8);
        video(&mut dos, 0x0d, 0, 0, 0);
        assert_ne!(
            dos.cpu().context().memory.read_u32(0x10c).unwrap(),
            0x7000_1000
        );
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 8);
    });
}

#[test]
fn invalid_or_unterminated_font_overrides_leave_the_mode_defaults() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        install(&mut dos);
        let before = dos.vga().borrow().snapshot();
        {
            let mut context = dos.cpu().context();
            context
                .memory
                .write_wrapping(PRIMARY.wrapping_add(8), &0x7000_0100u32.to_le_bytes())
                .unwrap();
            context
                .memory
                .write(0x70100, &[33, 0, 1, 0, 65, 0, 0, 0, 0, 0x80, 12, 3, 0xff])
                .unwrap();
        }
        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(dos.vga().borrow().snapshot().plane(2), before.plane(2));
        assert_eq!(dos.cpu().context().memory.read_u16(0x485).unwrap(), 16);
        dos.cpu()
            .context()
            .memory
            .write(0x70000, &[0x40; 0x10000])
            .unwrap();
        video(&mut dos, 3, 0, 0, 0); // No matching mode or FF terminator in the segment.
        assert_eq!(dos.cpu().context().memory.read_u8(0x484).unwrap(), 24);
        assert_eq!(information(&mut dos, 0x4000, 0x200, 0)[0x32], 4);
    });
}
