use super::*;

#[test]
fn vbe_cursor_preserves_banks_reserved_bits_and_restores_pixels_across_a_bank_boundary() {
    with_mouse(async |_, mut dos| {
        for (mode, bytes, original, inverted, black, white) in [
            (0x101, 1, 0xa5u32, 0xaa, 0, 15),
            (0x111, 2, 0x5aa5, 0xa55a, 0, 0xffff),
            (0x142, 4, 0x7a123456, 0x7aedcba9, 0x7a000000, 0x7affffff),
        ] {
            video(&mut dos, 0x4f02, mode, 0, 0).await;
            assert_eq!(dos.cpu().cpu_state().ax(), 0x4f);
            video(&mut dos, 0x4f06, 0, 1024, 0).await;
            video(&mut dos, 0x4f07, 0, 2, 1).await;
            call(&mut dos, 0, 0, 0, 0).await;
            call(&mut dos, 3, 0, 0, 0).await;
            assert_eq!(
                (dos.cpu().cpu_state().cx(), dos.cpu().cpu_state().dx()),
                (320, 240)
            );
            let io = dos.cpu().context().io.clone();
            let memory = dos.cpu().memory().with_io(io.clone());
            let y = 65536 / (1024 * bytes) - 2;
            for row in 0..2 {
                // Two adjacent cursor rows lie on opposite sides of a CPU bank boundary.
                for x in 10..14 {
                    let address = ((y + row + 1) * 1024 + x + 2) * bytes;
                    video(&mut dos, 0x4f05, 0, 0, (address / 65536) as u16).await;
                    memory
                        .view()
                        .write(
                            0xa0000 + address % 65536,
                            &original.to_le_bytes()[..bytes as usize],
                        )
                        .unwrap();
                }
            }
            call(&mut dos, 4, 0, 10, y as u16).await;
            let mut screen = [0xffff; 16];
            let mut cursor = [0; 16];
            screen[..2].fill(0xcfff);
            cursor[..2].fill(0x5000);
            super::graphics_cursor::shape(&mut dos, [0, 0], screen, cursor).await;
            video(&mut dos, 0x4f05, 0, 0, 7).await;
            let before = dos.vga().borrow().dispi_state();
            call(&mut dos, 1, 0, 0, 0).await;
            assert_eq!(dos.vga().borrow().dispi_state(), before);
            for row in 0..2 {
                for (x, expected) in (10..14).zip([original, inverted, black, white]) {
                    let address = ((y + row + 1) * 1024 + x + 2) * bytes;
                    video(&mut dos, 0x4f05, 0, 0, (address / 65536) as u16).await;
                    let mut value = [0; 4];
                    memory
                        .view()
                        .read(0xa0000 + address % 65536, &mut value[..bytes as usize])
                        .unwrap();
                    assert_eq!(u32::from_le_bytes(value), expected);
                }
            }
            call(&mut dos, 2, 0, 0, 0).await;
            for row in 0..2 {
                let address = ((y + row + 1) * 1024 + 13) * bytes;
                video(&mut dos, 0x4f05, 0, 0, (address / 65536) as u16).await;
                let mut value = [0; 4];
                memory
                    .view()
                    .read(0xa0000 + address % 65536, &mut value[..bytes as usize])
                    .unwrap();
                assert_eq!(u32::from_le_bytes(value), original);
            }
        }
    });
}
