use super::*;
use relocade_bios::video;
use relocade_machine::{FarPointer, IoBus, MemoryBus};

fn graphics(dos: &mut Dos) -> (MemoryBus, IoBus) {
    let io = dos.cpu().context().io.clone();
    let memory = dos.cpu().memory().with_io(io.clone());
    video::set_mode(&mut memory.view(), &io, 0x13).unwrap();
    (memory, io)
}

pub(super) async fn shape(dos: &mut Dos, hotspot: [i16; 2], screen: [u16; 16], cursor: [u16; 16]) {
    // The 64-byte ABI buffer crosses the real-mode offset boundary.
    let source = FarPointer {
        segment: 0x2200,
        offset: 0xfff0,
    };
    let bytes: Vec<_> = screen
        .into_iter()
        .chain(cursor)
        .flat_map(u16::to_le_bytes)
        .collect();
    dos.cpu()
        .context()
        .memory
        .write_wrapping(source, &bytes)
        .unwrap();
    dos.cpu().cpu_state().set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, source.segment),
    );
    call(dos, 9, hotspot[0] as u16, hotspot[1] as u16, source.offset).await;
}

#[test]
fn mode13_masks_preserve_palette_bits_and_restore_on_move_hide_and_shape_change() {
    with_mouse(async |_, mut dos| {
        let (memory, io) = graphics(&mut dos);
        for x in 10..14 {
            video::write_pixel(&mut memory.view(), &io, 0, x, 10, 0xa5).unwrap();
        }
        call(&mut dos, 4, 0, 20, 10).await;
        let mut screen = [0xffff; 16];
        let mut cursor = [0; 16];
        screen[0] = 0xcfff; // transparent, invert, black, white.
        cursor[0] = 0x5000;
        shape(&mut dos, [0, 0], screen, cursor).await;
        call(&mut dos, 1, 0, 0, 0).await;
        let pixels: Vec<_> = (10..14)
            .map(|x| video::read_pixel(&memory.view(), &io, 0, x, 10).unwrap())
            .collect();
        assert_eq!(pixels, [0xa5, 0xaa, 0, 15]);
        // The transparent cell is not part of the saved background.
        video::write_pixel(&mut memory.view(), &io, 0, 10, 10, 0x96).unwrap();
        packet(&mut dos, [8, 2, 0]).await; // Two logical X units are one display pixel.
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 10, 10),
            Some(0x96)
        );
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 11, 10),
            Some(0xa5)
        );
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 12, 10),
            Some(0xaa)
        );
        shape(&mut dos, [0, 0], [0xffff; 16], [0; 16]).await;
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 12, 10),
            Some(0xa5)
        );
        shape(&mut dos, [0, 0], screen, cursor).await;
        call(&mut dos, 2, 0, 0, 0).await;
        let pixels: Vec<_> = (10..14)
            .map(|x| video::read_pixel(&memory.view(), &io, 0, x, 10).unwrap())
            .collect();
        assert_eq!(pixels, [0x96, 0xa5, 0xa5, 0xa5]);
    });
}

#[test]
fn guest_copies_include_the_cursor_and_overdrawing_lasts_until_a_mouse_update() {
    with_mouse(async |clock, mut dos| {
        let (memory, io) = graphics(&mut dos);
        memory.view().write_u8(0xa0000, 0x2f).unwrap();
        memory.view().write_u8(0xa0001, 0x36).unwrap();
        call(&mut dos, 4, 0, 0, 0).await;
        let mut screen = [0xffff; 16];
        let mut cursor = [0; 16];
        screen[0] = 0x7fff;
        cursor[0] = 0x8000;
        shape(&mut dos, [0, 0], screen, cursor).await;
        call(&mut dos, 1, 0, 0, 0).await;
        let copied = memory.view().read_u8(0xa0000).unwrap();
        assert_eq!(copied, 15);
        memory.view().write_u8(0xa0100, copied).unwrap();

        memory.view().write_u8(0xa0000, 0x47).unwrap();
        clock.advance_by(Duration::from_millis(33));
        io.read_port(0x3da, 1); // Publish the next frame without repainting the cursor.
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa0000).unwrap(), 0x47);

        packet(&mut dos, [8, 2, 0]).await;
        assert_eq!(memory.view().read_u8(0xa0000).unwrap(), 0x2f);
        assert_eq!(memory.view().read_u8(0xa0001).unwrap(), 15);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa0001).unwrap(), 0x36);
        assert_eq!(memory.view().read_u8(0xa0100).unwrap(), 15);
    });
}

#[test]
fn mode13_hotspots_clip_at_both_edges_without_touching_adjacent_rows() {
    with_mouse(async |_, mut dos| {
        let (memory, io) = graphics(&mut dos);
        video::write_pixel(&mut memory.view(), &io, 0, 0, 0, 0x25).unwrap();
        video::write_pixel(&mut memory.view(), &io, 0, 319, 199, 0x36).unwrap();
        call(&mut dos, 4, 0, 0, 0).await;
        shape(&mut dos, [15, 15], [0; 16], [0xffff; 16]).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 0, 0), Some(15));
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 1, 0), Some(0));
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 0, 1), Some(0));
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 0, 0), Some(0x25));
        // Negative hotspot shifts right/down, leaving exactly one visible pixel.
        call(&mut dos, 4, 0, 636, 198).await;
        shape(&mut dos, [-1, -1], [0; 16], [0xffff; 16]).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 319, 199),
            Some(15)
        );
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 318, 199), Some(0));
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 319, 198), Some(0));
        call(&mut dos, 0, 0, 0, 0).await;
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 319, 199),
            Some(0x36)
        );
        // Reset supplies a visible default arrow under the BIOS palette.
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(
            video::read_pixel(&memory.view(), &io, 0, 161, 101),
            Some(15)
        );
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 161, 101), Some(0));
    });
}

#[test]
fn mode13_process_exit_restores_graphics_and_hidden_mode_changes_do_not_replay_old_pixels() {
    with_mouse(async |_, mut dos| {
        let (memory, io) = graphics(&mut dos);
        call(&mut dos, 4, 0, 0, 0).await;
        shape(&mut dos, [0, 0], [0; 16], [0xffff; 16]).await;
        video::write_pixel(&mut memory.view(), &io, 0, 0, 0, 0x42).unwrap();
        call(&mut dos, 1, 0, 0, 0).await;
        call(&mut dos, 2, 0, 0, 0).await;
        video::set_mode(&mut memory.view(), &io, 0x13).unwrap();
        call(&mut dos, 1, 0, 0, 0).await;
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        memory
            .view()
            .write(base + 0x100, &[0xb8, 0, 0x4c, 0xcd, 0x21])
            .unwrap();
        dos.cpu().cpu_state().set_eip(0x100);
        assert_eq!(
            dos.cpu().run().await,
            ExecutionExit::Host(relocade_x86::HostExit::Return)
        );
        assert_eq!(video::read_pixel(&memory.view(), &io, 0, 0, 0), Some(0));
    });
}
