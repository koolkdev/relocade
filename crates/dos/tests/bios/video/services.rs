use super::{video, PROGRAM};
use crate::common;
use relocade_bios::video::{
    self as bios, CharacterCell, CharacterRun, CursorState, ModeInfo, ScrollDirection, Window,
};

#[test]
fn rust_video_services_and_interrupts_share_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let (clock, timers) = relocade_machine::timer_service();
        std::thread::spawn(|| pollster::block_on(timers.run()));
        let teletype = relocade_bios::Teletype::new(
            dos.cpu().memory().with_io(dos.cpu().context().io.clone()),
            std::rc::Rc::new(clock),
        );
        {
            let mut context = dos.cpu().context();
            assert_eq!(
                bios::set_mode(&mut context.memory, context.io, 3),
                Some(0x30)
            );
            bios::set_cursor_shape(&mut context.memory, context.io, 0x0607).unwrap();
            bios::set_cursor(&mut context.memory, context.io, 2, 3, 4).unwrap();
            bios::write_characters(
                &mut context.memory,
                context.io,
                CharacterRun {
                    page: 2,
                    character: b'R',
                    color: 0x2e,
                    count: 2,
                },
                true,
            )
            .unwrap();
            assert_eq!(context.memory.read_u16(0xba1e8).unwrap(), 0x2e52);
            assert_eq!(context.memory.read_u16(0xba1ea).unwrap(), 0x2e52);
        }
        let cursor = video(&mut dos, 0x0300, 0x0200, 0, 0);
        assert_eq!(cursor.registers.edx as u16, 0x0304);
        assert_eq!(cursor.registers.ecx as u16, 0x0607);
        assert_eq!(
            video(&mut dos, 0x0800, 0x0200, 0, 0).registers.eax as u16,
            0x2e52
        );
        video(&mut dos, 0x0200, 0x0200, 0, 0x0305);
        video(&mut dos, 0x0a47, 0x0200, 1, 0); // Preserve the Rust-written attribute.
        {
            let mut context = dos.cpu().context();
            assert_eq!(
                bios::cursor(&context.memory, 2),
                Some(CursorState {
                    row: 3,
                    column: 5,
                    shape: 0x0607
                })
            );
            assert_eq!(
                bios::read_character(&context.memory, context.io, 2),
                Some(CharacterCell {
                    character: b'G',
                    attribute: 0x2e
                })
            );
            assert_eq!(
                bios::set_cursor(&mut context.memory, context.io, 8, 1, 1),
                None
            );
            assert_eq!(bios::cursor(&context.memory, 8), None);
            bios::select_page(&mut context.memory, context.io, 2).unwrap();
            assert_eq!(
                bios::mode(&context.memory),
                ModeInfo {
                    mode: 3,
                    columns: 80,
                    page: 2
                }
            );
            bios::scroll(
                &mut context.memory,
                context.io,
                Window {
                    top: 3,
                    left: 4,
                    bottom: 3,
                    right: 5,
                },
                0,
                0x1e,
                ScrollDirection::Up,
            );
            assert_eq!(context.memory.read_u16(0xba1e8).unwrap(), 0x1e20);
            teletype.write_character(b'T', 0).await.unwrap();
            assert_eq!(context.memory.read_u16(0xba1ea).unwrap(), 0x1e54);
        }
        assert_eq!(
            video(&mut dos, 0x0300, 0x0200, 0, 0).registers.edx as u16,
            0x0306
        );
        let mode = video(&mut dos, 0x0f00, 0x00a5, 0, 0);
        assert_eq!(mode.registers.eax as u16, 0x5003);
        assert_eq!(mode.registers.ebx as u16, 0x02a5);
    });
}
