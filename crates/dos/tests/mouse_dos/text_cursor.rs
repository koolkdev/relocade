use super::*;

#[test]
fn text_cursor_nests_visibility_applies_masks_and_restores_cells_on_motion_and_reset() {
    with_mouse(async |_, mut dos| {
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory().with_io(io);
        memory.view().write_u16(0xb8142, 0x1e41).unwrap(); // Row 2, column 1.
        memory.view().write_u16(0xb8144, 0x2f42).unwrap();
        let bios_cursor = memory.view().read_u16(0x450).unwrap();
        call(&mut dos, 4, 0, 8, 16).await;
        call(&mut dos, 2, 0, 0, 0).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x1e41);
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x6941);
        call(&mut dos, 1, 0, 0, 0).await; // Repeated show does not create an extra hide obligation.
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x1e41);
        call(&mut dos, 1, 0, 0, 0).await;
        call(&mut dos, 0xa, 0, 0x00ff, 0x4f00).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x4f41);
        packet(&mut dos, [8, 8, 0]).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x1e41);
        assert_eq!(memory.view().read_u16(0xb8144).unwrap(), 0x4f42);
        assert_eq!(memory.view().read_u16(0x450).unwrap(), bios_cursor);
        call(&mut dos, 0, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8144).unwrap(), 0x2f42);
    });
}

#[test]
fn text_cursor_clips_offscreen_positions_and_restores_the_original_page_on_exit() {
    with_mouse(async |_, mut dos| {
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory().with_io(io.clone());
        memory.view().write_u16(0xb8000, 0x1e41).unwrap();
        memory.view().write_u16(0xb9000, 0x2f42).unwrap();
        call(&mut dos, 4, 0, 0, 0).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8000).unwrap(), 0x6941);
        // Selecting another page does not change the layout of the saved page.
        relocade_bios::video::select_page(&mut memory.view(), &io, 1).unwrap();
        assert_eq!(memory.view().read_u16(0xb8000).unwrap(), 0x6941);
        assert_eq!(memory.view().read_u16(0xb9000).unwrap(), 0x2f42);
        call(&mut dos, 4, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8000).unwrap(), 0x1e41);
        assert_eq!(memory.view().read_u16(0xb9000).unwrap(), 0x5842);
        call(&mut dos, 7, 0, (-8i16) as u16, 800).await;
        call(&mut dos, 4, 0, 800, 0).await;
        assert_eq!(memory.view().read_u16(0xb9000).unwrap(), 0x2f42);
        call(&mut dos, 4, 0, (-8i16) as u16, 0).await;
        assert_eq!(memory.view().read_u16(0xb9000).unwrap(), 0x2f42);
        call(&mut dos, 4, 0, 0, 0).await;
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
        assert_eq!(memory.view().read_u16(0xb9000).unwrap(), 0x2f42);
    });
}
