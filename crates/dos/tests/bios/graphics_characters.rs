use super::video::{video, PROGRAM};
use crate::common;

#[test]
fn two_color_planar_characters_force_white_only_for_write_with_attribute() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0011, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x43 * 4, 0x2300_0000).unwrap();
            context
                .memory
                .write(0x23000 + 65 * 16, &[0x81; 16])
                .unwrap();
        }
        video(&mut dos, 0x0941, 0, 1, 0); // Even BL=0 draws white.
        let white = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0941, 0x0080, 1, 0); // XOR toggles that same white.
        let erased = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a41, 4, 1, 0); // AH=0Ah retains the low color bits.
        let red_plane = dos.vga().borrow().snapshot();
        for plane in 0..4 {
            for y in 0..16 {
                assert_eq!(white.plane(plane)[y * 80], 0x81);
                assert_eq!(erased.plane(plane)[y * 80], 0);
                assert_eq!(
                    red_plane.plane(plane)[y * 80],
                    if plane == 2 { 0x81 } else { 0 }
                );
            }
        }
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0);
        video(&mut dos, 0x0e41, 0, 0, 0); // Teletype also respects foreground zero.
        for plane in 0..4 {
            assert_eq!(dos.vga().borrow().snapshot().plane(plane)[0], 0);
        }
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 1);
    });
}

#[test]
fn planar_characters_replace_or_xor_glyph_bits_without_moving_the_cursor() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x000d, 0, 0, 0);
        video(&mut dos, 0x0507, 0, 0, 0);
        video(&mut dos, 0x0600, 0x0500, 0, 0xffff);
        video(&mut dos, 0x0500, 0, 0, 0); // Repeated writes still target BH's hidden page.
        video(&mut dos, 0x0200, 0x0700, 0, 39);
        let glyph = [0x81, 0x42, 0x24, 0x18, 0xff, 0, 0x80, 1];
        {
            let mut context = dos.cpu().context();
            // Character 7 starts at 2000:FFFC and crosses the segment offset.
            context.memory.write_u32(0x43 * 4, 0x2000_ffc4).unwrap();
            context.memory.write(0x2fffc, &glyph[..4]).unwrap();
            context.memory.write(0x20000, &glyph[4..]).unwrap();
            for (port, word) in [
                (0x3c4, 0x0502),
                (0x3ce, 0x1b03),
                (0x3ce, 0x0304),
                (0x3ce, 0x0b05),
                (0x3ce, 0x5a08),
                (0x3ce, 0x0f01),
            ] {
                context.io.write_port(port, 2, word);
            }
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a07, 0x0783, 3, 0);
        let xor_background = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a07, 0x0783, 3, 0); // Undo XOR before replacement.
        video(&mut dos, 0x0907, 0x070e, 3, 0); // BEL is a literal glyph for AH=09h/0Ah.
        let replacement = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a07, 0x0783, 3, 0);
        let xor = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a07, 0x0783, 3, 0);
        let restored = dos.vga().borrow().snapshot();
        assert_eq!(restored.registers, before.registers);
        assert_eq!(dos.cpu().context().memory.read_u16(0x45e).unwrap(), 39);
        for plane in 0..4 {
            let mut expected = before.plane(plane).to_vec();
            let mut expected_xor = expected.clone();
            let mut expected_background = expected.clone();
            for (row, column) in [(0, 39), (1, 0), (1, 1)] {
                for (scanline, &bits) in glyph.iter().enumerate() {
                    let offset = 0xe000 + (row * 8 + scanline) * 40 + column;
                    expected[offset] = if 14 & (1 << plane) != 0 { bits } else { 0 };
                    expected_xor[offset] = if 13 & (1 << plane) != 0 { bits } else { 0 };
                    if plane < 2 {
                        expected_background[offset] ^= bits;
                    }
                }
            }
            assert_eq!(replacement.plane(plane), expected);
            assert_eq!(xor_background.plane(plane), expected_background);
            assert_eq!(xor.plane(plane), expected_xor);
            assert_eq!(restored.plane(plane), expected);
        }
        // Even a maximal repeat stops at the graphics page bottom, without
        // wrapping into another page or advancing the stored cursor.
        video(&mut dos, 0x0200, 0x0700, 0, 0x1827);
        video(&mut dos, 0x0907, 0x0702, 0xffff, 0);
        let bottom = dos.vga().borrow().snapshot();
        for plane in 0..4 {
            let mut expected = restored.plane(plane).to_vec();
            for (scanline, &bits) in glyph.iter().enumerate() {
                expected[0xe000 + (192 + scanline) * 40 + 39] = if plane == 1 { bits } else { 0 };
            }
            assert_eq!(bottom.plane(plane), expected);
        }
        assert_eq!(dos.cpu().context().memory.read_u16(0x45e).unwrap(), 0x1827);
    });
}

#[test]
fn indexed_characters_use_the_full_int43_font_and_keep_high_color_bits() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0013, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x43 * 4, 0x2300_0000).unwrap();
            context.memory.write_u32(0x1f * 4, 0x2400_0000).unwrap();
            context.memory.write(0x23000 + 65 * 8, &[0x80; 8]).unwrap();
            context.memory.write(0x23000 + 128 * 8, &[0x01; 8]).unwrap();
            context.memory.write(0x24000, &[0xff; 8]).unwrap(); // CGA-only vector is ignored.
            context.memory.write(0xa0000, &[0x55; 65536]).unwrap();
        }
        video(&mut dos, 0x0200, 0, 0, 0x0102);
        video(&mut dos, 0x0a41, 0x0085, 1, 0);
        video(&mut dos, 0x0200, 0, 0, 0x0103);
        video(&mut dos, 0x0980, 0x00f1, 1, 0);
        let mut expected = vec![0x55; 65536];
        for y in 8..16 {
            expected[y * 320 + 16..y * 320 + 32].fill(0);
            expected[y * 320 + 16] = 0x85;
            expected[y * 320 + 31] = 0xf1;
        }
        let mut actual = vec![0; 65536];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x0103);
        // This VGA BIOS uses BH as a page, not the ET4000 background extension.
        assert_eq!(
            video(&mut dos, 0x0941, 0x0107, 1, 0).registers.eax as u16,
            0x8641
        );
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
    });
}

#[test]
fn tall_graphics_fonts_use_int43_and_read_vram_before_remapping_access() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0012, 0, 0, 0);
        let glyph = [
            0x81, 0x42, 0x24, 0x18, 0xff, 0, 0x80, 1, 0xf0, 0x0f, 0xaa, 0x55, 3, 6, 12, 24,
        ];
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x43 * 4, 0xa000_0800).unwrap(); // Character 80h at A000:1000.
            context.memory.write_u32(0x1f * 4, 0x2400_0000).unwrap();
            context.memory.write(0x24000, &[255; 8]).unwrap();
            context.io.write_port(0x3c4, 2, 0x0402); // Glyph in plane two.
            context.memory.write(0xa1000, &glyph).unwrap();
            // Read mode one returns plane-two matches. Reading after the BIOS
            // switches to read mode zero would instead return empty plane zero.
            for word in [0x0402, 0x0407, 0x0004, 0x0805] {
                context.io.write_port(0x3ce, 2, word);
            }
        }
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0a80, 9, 1, 0);
        let after = dos.vga().borrow().snapshot();
        assert_eq!(after.registers, before.registers);
        for plane in 0..4 {
            let mut expected = before.plane(plane).to_vec();
            for (y, &byte) in glyph.iter().enumerate() {
                expected[y * 80] = if plane == 0 || plane == 3 { byte } else { 0 };
            }
            assert_eq!(after.plane(plane), expected);
        }
    });
}

#[test]
fn graphics_teletype_wraps_and_scrolls_the_bios_grid_with_black_fill() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x0013, 0, 0, 0);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u32(0x43 * 4, 0x2300_0000).unwrap();
            context.memory.write(0x23000 + 65 * 8, &[255; 8]).unwrap();
            context.memory.write(0x23000 + 66 * 8, &[0x80; 8]).unwrap();
            context.memory.write_u16(0x44a, 3).unwrap();
            context.memory.write_u8(0x484, 1).unwrap(); // A 3-column, 2-row BIOS window.
            context.memory.write(0xa0000, &[0x55; 65536]).unwrap();
        }
        video(&mut dos, 0x0200, 0, 0, 0x0102);
        let before = dos.vga().borrow().snapshot();
        video(&mut dos, 0x0e41, 0xff85, 0, 0); // AH=0Eh uses the active page, not BH.
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x0100);
        assert_eq!(dos.vga().borrow().snapshot().registers, before.registers);
        let mut expected = vec![0x55; 65536];
        for y in 0..8 {
            expected[y * 320 + 16..y * 320 + 24].fill(0x85);
        }
        for y in 8..16 {
            expected[y * 320..y * 320 + 24].fill(0);
        }
        let mut actual = vec![0; 65536];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        for (character, cursor) in [
            (7, 0x0100),
            (8, 0x0100),
            (b'B', 0x0101),
            (8, 0x0100),
            (b'\r', 0x0100),
        ] {
            video(&mut dos, 0x0e00 | u16::from(character), 4, 0, 0);
            assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), cursor);
        }
        for y in 8..16 {
            expected[y * 320] = 4;
        }
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected); // Backspace does not erase.
        video(&mut dos, 0x0e0a, 7, 0, 0);
        for y in 0..16 {
            expected[y * 320..y * 320 + 24].fill(0);
            if y < 8 {
                expected[y * 320] = 4;
            }
        }
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(dos.cpu().context().memory.read_u16(0x450).unwrap(), 0x0100);
    });
}
