use super::{common, video, PROGRAM};

#[test]
fn scanline_selection_and_font_height_support_twenty_five_forty_three_and_fifty_rows() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (selection, height, rows, control) in
            [(1, 14, 43, 0), (2, 16, 50, 0x10), (0, 8, 50, 0x80)]
        {
            let before = dos.vga().borrow().snapshot();
            assert_eq!(
                video(&mut dos, 0x1200 | selection, 0x30, 0, 0)
                    .registers
                    .eax as u16,
                0x1212
            );
            assert_eq!(before.registers, dos.vga().borrow().snapshot().registers);
            {
                let mut context = dos.cpu().context();
                context.memory.write_u16(0xb8000, 0x2e41).unwrap();
                context.memory.write_u16(0xb9ffe, 0x4f5a).unwrap();
                assert_eq!(context.memory.read_u8(0x489).unwrap() & 0x90, control);
            }
            video(&mut dos, 0x0083, 0, 0, 0); // Apply the preference without clearing text.
            {
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), height);
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x1000);
                assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x2e41);
                assert_eq!(context.memory.read_u16(0xb9ffe).unwrap(), 0x4f5a);
            }
            let preset = dos.vga().borrow().snapshot();
            video(&mut dos, 0x1112, 0, 0, 0);
            let expanded = dos.vga().borrow().snapshot();
            assert_eq!(expanded.registers.crtc.values[9], 0x47);
            for register in [0, 1, 6, 7, 0x10, 0x11, 0x12, 0x15, 0x16, 0x17] {
                assert_eq!(
                    expanded.registers.crtc.values[register],
                    preset.registers.crtc.values[register]
                );
            }
            {
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u8(0x484).unwrap(), rows - 1);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), 8);
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x2000);
                assert_eq!(context.memory.read_u16(0x460).unwrap(), 0x0607);
                assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x2e41);
            }
            // Page selection and character access consume the new BIOS allocation.
            video(&mut dos, 0x0200, 0x0300, 0, (u16::from(rows) - 1) << 8);
            video(&mut dos, 0x0951, 0x031e, 1, 0);
            video(&mut dos, 0x0503, 0, 0, 0);
            let capture = common::next_capture(&dos).await;
            assert_eq!(capture.display.registers.crtc.display_start, 0x3000);
            assert_eq!(
                video(&mut dos, 0x0800, 0x0300, 0, 0).registers.eax as u16,
                0x1e51
            );
            assert_eq!(
                video(&mut dos, 0x0504, 0, 0, 0).registers.eax as u16,
                0x8604
            );
            video(&mut dos, 0x0500, 0, 0, 0);
            // Keeping VDE allows 350/8 -> 350/14 and 400/8 -> 400/16 to return to 25 rows.
            if height != 8 {
                video(
                    &mut dos,
                    if height == 14 { 0x1111 } else { 0x1114 },
                    0,
                    0,
                    0,
                );
                let context = dos.cpu().context();
                assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
                assert_eq!(context.memory.read_u16(0x485).unwrap(), height);
                assert_eq!(context.memory.read_u16(0x44c).unwrap(), 0x1000);
            }
        }
    });
}

#[test]
fn geometry_changing_font_calls_validate_before_upload_and_handle_monochrome_crtc_ports() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0507, 0, 0, 0);
        let before = common::next_capture(&dos).await;
        assert_eq!(
            video(&mut dos, 0x1114, 0, 0, 0).registers.eax as u16,
            0x8614
        );
        let after = dos.vga().borrow().snapshot();
        assert_eq!(before.display.registers, after.registers);
        assert_eq!(before.display.plane(2), after.plane(2));
        video(&mut dos, 0x0500, 0, 0, 0);
        // A one-high font would need 400 BIOS rows, beyond the byte-sized row field.
        assert_eq!(
            video(&mut dos, 0x1110, 0x0100, 1, 0).registers.eax as u16,
            0x8610
        );
        {
            let context = dos.cpu().context();
            context.io.write_port(0x3c2, 1, 0x42); // Same text layout, monochrome ports.
        }
        video(&mut dos, 0x1110, 0x1000, 0, 0); // Zero glyphs can still change the height.
        let snapshot = dos.vga().borrow().snapshot();
        assert_eq!(snapshot.registers.crtc.values[9], 0x4f);
        assert_eq!(&snapshot.registers.crtc.values[10..12], &[14, 15]);
        assert_eq!(snapshot.registers.miscellaneous, 0x42);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u8(0x484).unwrap(), 24);
        assert_eq!(context.memory.read_u16(0x485).unwrap(), 16);
        assert_eq!(context.memory.read_u16(0x460).unwrap(), 0x0e0f);
    });
}
