use super::*;

async fn install(dos: &mut MouseDos) -> u32 {
    let segment = dos.process().unwrap().psp_segment();
    let base = u32::from(segment) << 4;
    dos.cpu()
        .context()
        .memory
        .write(
            base + 0x300,
            &[
                0x2e, 0xa3, 0, 5, // AX event mask.
                0x2e, 0x89, 0x1e, 2, 5, // BX buttons.
                0x2e, 0x89, 0x0e, 4, 5, // CX position X.
                0x2e, 0x89, 0x16, 6, 5, // DX position Y.
                0x2e, 0x89, 0x36, 8, 5, // SI mickeys X.
                0x2e, 0x89, 0x3e, 10, 5, // DI mickeys Y.
                0x2e, 0xff, 6, 12, 5, // Count calls.
                0xb8, 0, 5, 0xb9, b'K', 0x25, 0xcd, 0x16, // Nest BIOS keyboard service.
                0xb8, 0xad, 0xde, 0xcb, // Clobber AX; RETF.
            ],
        )
        .unwrap();
    dos.cpu().cpu_state().set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, segment),
    );
    call(dos, 0xc, 0, 0x7f, 0x300).await;
    base
}

#[test]
fn callbacks_use_registers_nest_guest_services_and_are_retired_on_process_exit() {
    with_mouse(async |runner, mut dos| {
        let base = install(&mut dos).await;
        let memory = dos.cpu().memory();
        let state = dos.cpu().cpu_state();
        call(&mut dos, 4, 0, 632, 192).await;
        let before = state.snapshot();
        use relocade_frontend::{
            keyboard::ElementState,
            mouse::{MouseButton, MouseEvent},
        };
        let input = dos.mouse_input();
        input
            .unbounded_send(MouseEvent::Button {
                button: MouseButton::Left,
                state: ElementState::Pressed,
            })
            .unwrap();
        input
            .unbounded_send(MouseEvent::Motion {
                delta_x: 16.0,
                delta_y: 16.0,
            })
            .unwrap();
        for _ in 0..4 {
            relocade_machine::yield_now().await;
        }
        runner.clock().advance(Duration::from_millis(10));
        for _ in 0..64 {
            common::run_until(dos.cpu(), 0x102).await;
            if memory.view().read_u16(base + 0x50c).unwrap() == 1 {
                break;
            }
            relocade_machine::yield_now().await;
        }
        let after = state.snapshot();
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.segments, before.segments);
        let mut words = [0; 14];
        memory.view().read(base + 0x500, &mut words).unwrap();
        assert_eq!(words, [3, 0, 1, 0, 120, 2, 192, 0, 16, 0, 16, 0, 1, 0]);
        // Exchange returns the old registration, and a zero mask prevents delivery.
        call(&mut dos, 0x14, 0, 0, 0).await;
        assert_eq!(
            (state.cx(), state.dx(), state.segment(Segment::Es).selector),
            (0x7f, 0x300, (base >> 4) as u16)
        );
        packet(&mut dos, [8, 0, 0]).await;
        assert_eq!(memory.view().read_u16(base + 0x50c).unwrap(), 1);
        install(&mut dos).await;
        memory
            .view()
            .write(base + 0x100, &[0xb8, 0, 0x4c, 0xcd, 0x21])
            .unwrap();
        state.set_eip(0x100);
        assert_eq!(
            dos.cpu().run().await,
            ExecutionExit::Host(relocade_x86::HostExit::Return)
        );
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        // Keep the old code and counter in RAM: a stale pointer would still run.
        state.set_eip(0x102);
        packet(&mut dos, [0x28, 2, 0xff]).await;
        assert_eq!(memory.view().read_u16(base + 0x50c).unwrap(), 1);
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(state.bx(), 0, "session state still receives packets");
    });
}

#[test]
fn mouse_events_resume_a_waiting_dos_read() {
    with_mouse(async |_, mut dos| {
        let base = install(&mut dos).await;
        let memory = dos.cpu().memory();
        memory
            .view()
            .write(base + 0x100, &[0xcd, 0x21, 0xeb, 0xfe])
            .unwrap();
        let state = dos.cpu().cpu_state();
        state.set_eip(0x100);
        state.set_ax(0x0700);
        state.set_eflags(0x443);
        let io = dos.cpu().context().io.clone();
        common::assert_pending(common::run_until(dos.cpu(), 0x102));
        for byte in [8, 1] {
            common::mouse::inject(&io, byte).await;
            common::assert_pending(common::run_until(dos.cpu(), 0x102));
        }
        common::mouse::inject(&io, 0).await;
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(state.al(), b'K');
        assert_eq!(state.sp(), 0xfffe);
        assert_eq!(state.eflags(), 0x443);
        assert_eq!(memory.view().read_u16(base + 0x50c).unwrap(), 1);
    });
}
