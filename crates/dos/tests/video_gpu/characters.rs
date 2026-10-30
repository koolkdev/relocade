use super::{common, run, Gpu};

#[test]
fn dos_and_bios_graphics_characters_share_guest_fonts_and_survive_mode_changes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let glyph = [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81];
        let mut program = vec![
            0xb8, 0x0d, 0, 0xcd, 0x10, // Mode 0Dh.
            0x31, 0xc0, 0x8e, 0xc0, // ES = 0 for font vector access.
            0x26, 0xc7, 0x06, 0x0c, 1, 0xf8, 0, // INT 43h offset: A's glyph at DS:0300.
            0x8c, 0xd8, 0x26, 0xa3, 0x0e, 1, // INT 43h segment = DS.
            0xb8, 2, 5, 0xcd, 0x10, // Display page two.
            0xb4, 2, 0xb2, b'A', 0xcd, 0x21, // DOS draws color 7 and advances.
            0xb8, b'A', 0x0e, 0xbb, 4, 7, 0xcd, 0x10, // BIOS draws red, ignoring BH.
            0xb8, b'A', 9, 0xbb, 2, 2, 0xb9, 2, 0, 0xcd, 0x10, // Two green glyphs.
            0xc3,
        ];
        program.resize(0x200, 0);
        program.extend_from_slice(&glyph);
        let capture = run(&mut dos, &program).await;
        assert_eq!(common::output(dos.stdout()), b"A");
        assert_eq!(dos.cpu().context().memory.read_u16(0x454).unwrap(), 2);
        let cleared = run(&mut dos, &[0xb8, 0x13, 0, 0xcd, 0x10, 0xc3]).await;
        drop(dos);
        let mut gpu = Gpu::new().await;
        let blank = gpu.render(&cleared, [320, 200]).await;
        assert!(blank
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255]));
        let image = gpu.render(&capture, [320, 200]).await;
        for y in 0..200 {
            for x in 0..320 {
                let color = if y < 8 && x < 32 && glyph[y] & (0x80 >> (x % 8)) != 0 {
                    match x / 8 {
                        0 => [170, 170, 170, 255],
                        1 => [170, 0, 0, 255],
                        _ => [0, 170, 0, 255],
                    }
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(&image[(y * 320 + x) * 4..][..4], &color, "({x}, {y})");
            }
        }
    });
}
