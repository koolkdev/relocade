use super::*;
use relocade_machine::FarPointer;

const BUFFER: FarPointer = FarPointer {
    segment: 0x8000,
    offset: 0xfff0,
};

async fn storage(dos: &mut Dos, function: u16, pointer: FarPointer) {
    dos.cpu().cpu_state().set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, pointer.segment),
    );
    // BX is deliberately zero: Microsoft's save/restore ABI does not require it.
    call(dos, function, 0, 0, pointer.offset).await;
}

#[test]
fn saved_state_relocates_and_restores_counters_fractional_motion_and_callbacks() {
    with_mouse(async |_, mut dos| {
        video(&mut dos, 0x13, 0, 0, 0).await;
        call(&mut dos, 0, 0, 0, 0).await;
        let state = dos.cpu().cpu_state();
        let memory = dos.cpu().memory();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        call(&mut dos, 7, 0, 0, 63).await;
        call(&mut dos, 8, 0, 0, 47).await;
        call(&mut dos, 4, 0, 20, 30).await;
        call(&mut dos, 15, 0, 20, 24).await;
        call(&mut dos, 0x1a, 50, 50, 0xffff).await;
        packet(&mut dos, [0x29, 1, 0xff]).await; // Fractional +0.4 X, +1/3 Y; left press.
        memory
            .view()
            .write(base + 0x300, &[0x2e, 0xff, 6, 0, 5, 0xcb])
            .unwrap();
        state.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, segment),
        );
        call(&mut dos, 12, 0, 7, 0x300).await;
        call(&mut dos, 0x15, 0, 0, 0).await;
        let size = usize::from(state.bx());
        assert_eq!(size, 172);
        let after = FarPointer {
            offset: BUFFER.offset.wrapping_add(size as u16),
            ..BUFFER
        };
        memory.view().write_u8(BUFFER.address() - 1, 0xa5).unwrap();
        memory.view().write_u8(after.address(), 0x5a).unwrap();
        storage(&mut dos, 0x16, BUFFER).await;
        assert_eq!(memory.view().read_u8(BUFFER.address() - 1).unwrap(), 0xa5);
        assert_eq!(memory.view().read_u8(after.address()).unwrap(), 0x5a);
        let mut bytes = vec![0; size];
        memory.view().read_wrapping(BUFFER, &mut bytes).unwrap();
        let copied = FarPointer {
            segment: 0x8100,
            offset: 0x100,
        };
        memory.view().write_wrapping(copied, &bytes).unwrap();
        memory
            .view()
            .write_wrapping(BUFFER, &vec![0; size])
            .unwrap();
        call(&mut dos, 0, 0, 0, 0).await;
        call(&mut dos, 0x1a, 99, 1, 10).await;
        storage(&mut dos, 0x17, copied).await;
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx(), state.dx()), (1, 20, 30));
        call(&mut dos, 5, 0, 0, 0).await;
        assert_eq!(
            (state.ax(), state.bx(), state.cx(), state.dx()),
            (1, 1, 20, 30)
        );
        call(&mut dos, 11, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), (1, 1));
        call(&mut dos, 0x1b, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx(), state.dx()), (50, 50, 0xffff));
        for _ in 0..2 {
            packet(&mut dos, [0x29, 1, 0xff]).await;
        }
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(
            (state.cx(), state.dx()),
            (20, 31),
            "saved Y fraction contributes"
        );
        for _ in 0..2 {
            packet(&mut dos, [0x29, 1, 0xff]).await;
        }
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(
            (state.cx(), state.dx()),
            (22, 31),
            "saved X fraction reaches the mode-13 grid"
        );
        assert_eq!(memory.view().read_u16(base + 0x500).unwrap(), 4);
        call(&mut dos, 4, 0, 1000, 1000).await;
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(
            (state.cx(), state.dx()),
            (62, 47),
            "saved bounds constrain movement"
        );
        call(&mut dos, 0x24, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx()), (0x0100, 0x0400));
    });
}

#[test]
fn restoring_cursor_state_rebuilds_background_and_preserves_nested_hides() {
    with_mouse(async |_, mut dos| {
        let memory = dos.cpu().memory().with_io(dos.cpu().context().io.clone());
        memory.view().write_u16(0xb8142, 0x1e41).unwrap();
        memory.view().write_u16(0xb8144, 0x2f42).unwrap();
        call(&mut dos, 4, 0, 8, 16).await;
        call(&mut dos, 10, 0, 0x00ff, 0x4f00).await;
        call(&mut dos, 1, 0, 0, 0).await;
        storage(&mut dos, 0x16, BUFFER).await;
        call(&mut dos, 2, 0, 0, 0).await;
        memory.view().write_u16(0xb8142, 0x3c43).unwrap();
        call(&mut dos, 4, 0, 16, 16).await;
        call(&mut dos, 10, 0, 0xffff, 0x7700).await;
        call(&mut dos, 1, 0, 0, 0).await;
        storage(&mut dos, 0x17, BUFFER).await;
        assert_eq!(memory.view().read_u16(0xb8144).unwrap(), 0x2f42);
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x4f43);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(
            memory.view().read_u16(0xb8142).unwrap(),
            0x3c43,
            "restoration saves the current background"
        );
        call(&mut dos, 2, 0, 0, 0).await;
        storage(&mut dos, 0x16, BUFFER).await; // Two outstanding hides.
        call(&mut dos, 1, 0, 0, 0).await;
        call(&mut dos, 1, 0, 0, 0).await;
        storage(&mut dos, 0x17, BUFFER).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x3c43);
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x4f43);
        // A malformed buffer leaves the live state and its saved background alone.
        // X ratio is at byte 12; visibility follows both axes,
        // motion/button counters and the callback at byte 98.
        for (offset, invalid) in [(12, 0), (98, 1)] {
            storage(&mut dos, 0x16, BUFFER).await;
            call(&mut dos, 4, 0, 16, 16).await;
            let field = FarPointer {
                offset: BUFFER.offset.wrapping_add(offset),
                ..BUFFER
            };
            memory.view().write_u16(field.address(), invalid).unwrap();
            storage(&mut dos, 0x17, BUFFER).await;
            assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x3c43);
            assert_eq!(memory.view().read_u16(0xb8144).unwrap(), 0x4f42);
            call(&mut dos, 4, 0, 8, 16).await;
        }
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u16(0xb8142).unwrap(), 0x3c43);
        assert_eq!(memory.view().read_u16(0xb8144).unwrap(), 0x2f42);
    });
}

#[test]
fn saved_graphics_masks_and_hotspot_apply_after_reset() {
    with_mouse(async |_, mut dos| {
        video(&mut dos, 0x13, 0, 0, 0).await;
        call(&mut dos, 0, 0, 0, 0).await;
        let memory = dos.cpu().memory().with_io(dos.cpu().context().io.clone());
        memory.view().write_u8(0xa0000, 0xa5).unwrap();
        let mut screen = [0xffff; 16];
        let mut cursor = [0; 16];
        screen[0] = 0x7fff;
        cursor[0] = 0x8000;
        graphics_cursor::shape(&mut dos, [1, 2], screen, cursor).await;
        call(&mut dos, 4, 0, 2, 2).await;
        call(&mut dos, 1, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa0000).unwrap(), 15);
        storage(&mut dos, 0x16, BUFFER).await;
        call(&mut dos, 0, 0, 0, 0).await;
        memory.view().write_u8(0xa0000, 0x36).unwrap();
        storage(&mut dos, 0x17, BUFFER).await;
        assert_eq!(memory.view().read_u8(0xa0000).unwrap(), 15);
        call(&mut dos, 2, 0, 0, 0).await;
        assert_eq!(memory.view().read_u8(0xa0000).unwrap(), 0x36);
    });
}
