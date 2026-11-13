use crate::common::VideoInspection;
use super::*;
use relocade_bios::video;
use relocade_machine::IoBus;

fn indexed(io: &IoBus, port: u16, register: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([register, value])));
}

#[test]
fn reset_uses_active_graphics_bounds_and_center_and_motion_reaches_the_last_row() {
    with_mouse(async |_, mut dos| {
        for (mode, height) in [(0x10, 350), (0x12, 480)] {
            video(&mut dos, mode, 0, 0, 0).await;
            call(&mut dos, 0, 0, 0, 0).await;
            call(&mut dos, 3, 0, 0, 0).await;
            let state = dos.cpu().cpu_state();
            assert_eq!((state.cx(), state.dx()), (320, height / 2));
            call(&mut dos, 4, 0, 639, height - 2).await;
            packet(&mut dos, [0x28, 0, 0xfc]).await; // Four downward mickeys give two logical rows.
            call(&mut dos, 3, 0, 0, 0).await;
            assert_eq!((state.cx(), state.dx()), (639, height - 1));
            call(&mut dos, 4, 0, 1000, 1000).await;
            call(&mut dos, 3, 0, 0, 0).await;
            assert_eq!((state.cx(), state.dx()), (639, height - 1));
        }
    });
}

#[test]
fn text_coordinates_follow_font_rows_and_forty_column_and_monochrome_pages() {
    with_mouse(async |_, mut dos| {
        for (mode, columns, base, x_step, scanlines, rows, center) in [
            (1, 40, 0xb8000, 16, 2, 50, 200),
            (7, 80, 0xb0000, 8, 2, 50, 200),
            (3, 80, 0xb8000, 8, 1, 43, 168),
        ] {
            video(&mut dos, 0x1200 | scanlines, 0x30, 0, 0).await;
            video(&mut dos, mode, 0, 0, 0).await;
            video(&mut dos, 0x1112, 0, 0, 0).await; // Eight-high font on 350/400 scanlines.
            video(&mut dos, 0x0501, 0, 0, 0).await;
            call(&mut dos, 0, 0, 0, 0).await;
            call(&mut dos, 3, 0, 0, 0).await;
            let state = dos.cpu().cpu_state();
            assert_eq!((state.cx(), state.dx()), (320, center));
            let io = dos.cpu().context().io.clone();
            let memory = dos.cpu().memory().with_io(io);
            let page = u32::from(memory.view().read_u16(0x44e).unwrap());
            let last = base + page + ((rows - 1) * columns + columns - 1) * 2;
            memory.view().write_u16(last, 0x1e51).unwrap();
            call(&mut dos, 4, 0, 639, (rows * 8 - 1) as u16).await;
            call(&mut dos, 3, 0, 0, 0).await;
            assert_eq!(
                (state.cx(), state.dx()),
                (640 - x_step, ((rows - 1) * 8) as u16)
            );
            call(&mut dos, 1, 0, 0, 0).await;
            assert_eq!(memory.view().read_u16(last).unwrap(), 0x6951);
            call(&mut dos, 2, 0, 0, 0).await;
            assert_eq!(memory.view().read_u16(last).unwrap(), 0x1e51);
        }
    });
}

#[test]
fn cga_and_planar_cursor_masks_are_visible_through_guest_video_access() {
    with_mouse(async |_, mut dos| {
        for (mode, original, inverted, white, logical_x) in
            [(4, 2, 1, 3, 20), (6, 1, 0, 1, 10), (0x10, 6, 9, 15, 10)]
        {
            video(&mut dos, mode, 0, 0, 0).await;
            call(&mut dos, 0, 0, 0, 0).await;
            let io = dos.cpu().context().io.clone();
            let memory = dos.cpu().memory().with_io(io.clone());
            for x in 10..14 {
                video::write_pixel(&mut memory.view(), &io, 0, x, 11, original).unwrap();
            }
            call(&mut dos, 4, 0, logical_x, 11).await;
            let mut screen = [0xffff; 16];
            let mut cursor = [0; 16];
            screen[0] = 0xcfff;
            cursor[0] = 0x5000;
            super::graphics_cursor::shape(&mut dos, [0, 0], screen, cursor).await;
            let core = dos.vga().borrow().core_state();
            call(&mut dos, 1, 0, 0, 0).await;
            assert_eq!(dos.vga().borrow().core_state(), core);
            let pixels: Vec<_> = (10..14)
                .map(|x| video::read_pixel(&memory.view(), &io, 0, x, 11).unwrap())
                .collect();
            assert_eq!(pixels, [original, inverted, 0, white]);
            call(&mut dos, 2, 0, 0, 0).await;
            for x in 10..14 {
                assert_eq!(
                    video::read_pixel(&memory.view(), &io, 0, x, 11),
                    Some(original)
                );
            }
        }
    });
}

#[test]
fn mode_x_cursor_uses_programmed_height_stride_page_and_reverse_alias_restoration() {
    with_mouse(async |_, mut dos| {
        video(&mut dos, 0x13, 0, 0, 0).await;
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory().with_io(io.clone());
        indexed(&io, 0x3c4, 4, 6);
        indexed(&io, 0x3d4, 0x11, 0);
        for (register, value) in [
            (6, 0x0b),
            (7, 0x3e),
            (0x12, 0xdf),
            (0x13, 48),
            (0x14, 0),
            (0x15, 0xe7),
            (0x16, 4),
            (0x17, 0xe3),
            (12, 0x40),
        ] {
            indexed(&io, 0x3d4, register, value);
        }
        call(&mut dos, 0, 0, 0, 0).await;
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(
            (dos.cpu().cpu_state().cx(), dos.cpu().cpu_state().dx()),
            (320, 120)
        );
        indexed(&io, 0x3c4, 2, 8);
        indexed(&io, 0x3ce, 4, 3);
        let address = 0xa0000 + 0x4000 + 239 * 96 + 1;
        memory.view().write_u8(address, 0xa5).unwrap();
        call(&mut dos, 4, 0, 14, 239).await;
        super::graphics_cursor::shape(&mut dos, [0, 0], [0; 16], [0xffff; 16]).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(address).unwrap(), 15);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(address).unwrap(), 0xa5);

        // Count-by-two aliases adjacent groups of four pixels within the cursor.
        indexed(&io, 0x3d4, 0x17, 0xeb);
        call(&mut dos, 4, 0, 0, 0).await;
        indexed(&io, 0x3c4, 2, 1);
        indexed(&io, 0x3ce, 4, 0);
        memory.view().write_u8(0xa4000, 0x96).unwrap();
        call(&mut dos, 1, 0, 0, 0).await;
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa4000).unwrap(), 0x96);
    });
}

#[test]
fn mode_x_cursor_uses_page_stride_and_restored_state_on_the_next_mouse_update() {
    with_mouse(async |_, mut dos| {
        video(&mut dos, 0x13, 0, 0, 0).await;
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory().with_io(io.clone());
        indexed(&io, 0x3c4, 4, 6);
        indexed(&io, 0x3d4, 0x14, 0);
        indexed(&io, 0x3d4, 0x17, 0xe3);
        indexed(&io, 0x3c4, 2, 1);
        indexed(&io, 0x3ce, 4, 0);
        for (offset, value) in [(80, 0x5a), (0x4000 + 80, 0x6b), (0x4000 + 96, 0x7c)] {
            memory.view().write_u8(0xa0000 + offset, value).unwrap();
        }
        call(&mut dos, 4, 0, 0, 1).await;
        let mut screen = [0xffff; 16];
        let mut cursor = [0; 16];
        screen[0] = 0x7fff;
        cursor[0] = 0x8000;
        super::graphics_cursor::shape(&mut dos, [0, 0], screen, cursor).await;
        call(&mut dos, 1, 0, 0, 0).await;
        let core = dos.vga().borrow().core_state();
        indexed(&io, 0x3d4, 12, 0x40);
        indexed(&io, 0x3d4, 0x13, 48);
        assert_eq!(memory.view().read_u8(0xa0050).unwrap(), 15);
        assert_eq!(memory.view().read_u8(0xa4050).unwrap(), 0x6b);
        assert_eq!(memory.view().read_u8(0xa4060).unwrap(), 0x7c);

        call(&mut dos, 4, 0, 0, 1).await;
        assert_eq!(memory.view().read_u8(0xa0050).unwrap(), 0x5a);
        assert_eq!(memory.view().read_u8(0xa4050).unwrap(), 0x6b);
        assert_eq!(memory.view().read_u8(0xa4060).unwrap(), 15);
        io.with_context(|context| {
            dos.vga()
                .borrow_mut()
                .restore_state(context, Some(core.clone()), None, None);
        });
        assert_eq!(dos.vga().borrow().core_state(), core);
        assert_eq!(memory.view().read_u8(0xa0050).unwrap(), 0x5a);
        assert_eq!(memory.view().read_u8(0xa4060).unwrap(), 15);

        packet(&mut dos, [8, 0, 0]).await;
        assert_eq!(memory.view().read_u8(0xa4060).unwrap(), 0x7c);
        assert_eq!(memory.view().read_u8(0xa0050).unwrap(), 15);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa0050).unwrap(), 0x5a);
    });
}

#[test]
fn text_cursor_uses_new_font_geometry_on_the_next_mouse_update() {
    with_mouse(async |_, mut dos| {
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory().with_io(io);
        let cell = 0xb8000 + 30 * 80 * 2;
        memory.view().write_u16(cell, 0x1e41).unwrap();
        call(&mut dos, 8, 0, 0, 399).await;
        call(&mut dos, 4, 0, 0, 240).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(cell).unwrap(), 0x1e41);
        video(&mut dos, 0x1112, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(cell).unwrap(), 0x1e41);
        packet(&mut dos, [8, 0, 0]).await;
        assert_eq!(memory.view().read_u16(cell).unwrap(), 0x6941);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(cell).unwrap(), 0x1e41);
    });
}
