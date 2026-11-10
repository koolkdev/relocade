#![cfg(not(target_arch = "wasm32"))]

use relocade_clock_test_support::ManualClock;
mod common;
use relocade_dos::Dos;
use relocade_machine::ExecutionWorker;
use relocade_x86::{ExecutionExit, Segment, SegmentRegister};
use std::time::Duration;

const PROGRAM: &[u8] = &[0xcd, 0x33, 0xeb, 0xfe];

fn with_mouse(check: impl AsyncFnOnce(&ManualClock, Dos) + Send + 'static) {
    let clock = ManualClock::new();
    let worker_clock = clock.clone();
    let worker = ExecutionWorker::spawn_with_clock(
        move || worker_clock.clock(),
        async move |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(PROGRAM, Default::default()).unwrap();
            call(&mut dos, 0, 0, 0, 0).await;
            assert_eq!(dos.cpu().cpu_state().ax(), 0xffff);
            assert_eq!(dos.cpu().cpu_state().bx(), 3);
            check(&clock, dos).await;
        },
    );
    assert_eq!(pollster::block_on(worker.join()).unwrap(), Some(()));
}

async fn call(dos: &mut Dos, ax: u16, bx: u16, cx: u16, dx: u16) {
    let state = dos.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(ax);
    state.set_bx(bx);
    state.set_cx(cx);
    state.set_dx(dx);
    state.set_eflags(0x643);
    assert_eq!(
        common::run_until(dos.cpu(), 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.sp(), 0xfffe);
    assert_eq!(state.eflags(), 0x643);
}

async fn packet(dos: &mut Dos, bytes: [u8; 3]) {
    let io = dos.cpu().context().io.clone();
    dos.cpu().cpu_state().set_interrupts_enabled(true);
    for byte in bytes {
        common::mouse::inject(&io, byte).await;
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
    }
}

async fn video(dos: &mut Dos, ax: u16, bx: u16, cx: u16, dx: u16) {
    let interrupt_byte = (u32::from(dos.process().unwrap().psp_segment()) << 4) + 0x101;
    dos.cpu()
        .context()
        .memory
        .write_u8(interrupt_byte, 0x10)
        .unwrap();
    call(dos, ax, bx, cx, dx).await;
    dos.cpu()
        .context()
        .memory
        .write_u8(interrupt_byte, 0x33)
        .unwrap();
}

#[test]
fn com_and_mz_discover_the_resident_driver_and_preserve_the_interrupt_abi() {
    for mz in [false, true] {
        let worker = ExecutionWorker::spawn_with_clock(
            || ManualClock::new().clock(),
            async move |runner| {
                let mut dos = Dos::new(runner).await.unwrap();
                if mz {
                    dos.launch_mz(&common::mz_image(PROGRAM), Default::default())
                        .unwrap();
                } else {
                    dos.launch_com(PROGRAM, Default::default()).unwrap();
                }
                let state = dos.cpu().cpu_state();
                let sp = state.sp();
                let end = state.eip() + 2;
                state.set_ax(0);
                state.set_cx(0x1234);
                state.set_dx(0x5678);
                state.set_si(0xabcd);
                state.set_eflags(0x443);
                let before = state.snapshot();
                assert_eq!(
                    common::run_until(dos.cpu(), end).await,
                    ExecutionExit::Interrupted
                );
                assert_eq!(
                    (state.ax(), state.bx(), state.cx(), state.dx(), state.si()),
                    (0xffff, 3, 0x1234, 0x5678, 0xabcd)
                );
                assert_eq!(state.sp(), sp);
                assert_eq!(state.eflags(), 0x443);
                assert_eq!(state.snapshot().segments, before.segments);
            },
        );
        assert_eq!(pollster::block_on(worker.join()).unwrap(), Some(()));
    }
}

#[test]
fn bounds_button_histories_and_raw_motion_have_independent_consumption() {
    with_mouse(async |_, mut dos| {
        let state = dos.cpu().cpu_state();
        call(&mut dos, 7, 0, 0, 63).await;
        call(&mut dos, 8, 0, 0, 31).await;
        call(&mut dos, 4, 0, 32, 16).await;
        packet(&mut dos, [9, 200, 0]).await; // +200 is positive in the nine-bit packet.
        packet(&mut dos, [8, 0, 0]).await;
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx(), state.dx()), (0, 56, 16));
        call(&mut dos, 5, 0, 0, 0).await;
        assert_eq!(
            (state.ax(), state.bx(), state.cx(), state.dx()),
            (0, 1, 56, 16)
        );
        call(&mut dos, 5, 0, 0, 0).await;
        assert_eq!(state.bx(), 0);
        call(&mut dos, 6, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx(), state.dx()), (1, 56, 16));
        packet(&mut dos, [0x3c, 0, 0xff]).await; // X=-256, Y=-1, middle press.
        call(&mut dos, 0xb, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), ((-56i16) as u16, 1));
        call(&mut dos, 0xb, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), (0, 0));
        call(&mut dos, 5, 2, 0, 0).await;
        assert_eq!(
            (state.ax(), state.bx(), state.cx(), state.dx()),
            (4, 1, 0, 16)
        );
    });
}

#[test]
fn ratios_sensitivity_and_timed_acceleration_leave_mickeys_unscaled() {
    with_mouse(async |clock, mut dos| {
        let state = dos.cpu().cpu_state();
        {
            let mut call = dos.cpu().context();
            relocade_bios::video::set_mode(&mut call.memory, call.io, 0x13).unwrap();
        }
        call(&mut dos, 4, 0, 100, 100).await;
        call(&mut dos, 0xf, 0, 16, 8).await;
        call(&mut dos, 0x1a, 75, 25, 100).await;
        call(&mut dos, 0x1b, 0, 0, 0).await;
        assert_eq!((state.bx(), state.cx(), state.dx()), (75, 25, 100));
        packet(&mut dos, [0x28, 8, 0xf8]).await;
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), (108, 103));
        clock.advance_to(clock.at(Duration::from_millis(10)));
        packet(&mut dos, [0x28, 8, 0xf8]).await; // 800 mickeys/s: doubled.
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), (126, 109));
        call(&mut dos, 0xb, 0, 0, 0).await;
        assert_eq!((state.cx(), state.dx()), (16, 16));
        // Half-logical-pixel motion survives packet boundaries and cancels correctly.
        call(&mut dos, 0x1a, 50, 50, 0xffff).await;
        call(&mut dos, 4, 0, 100, 100).await;
        for bytes in [
            [8, 1, 0],
            [0x18, 0xff, 0],
            [8, 1, 0],
            [8, 1, 0],
            [8, 1, 0],
            [8, 1, 0],
        ] {
            packet(&mut dos, bytes).await;
        }
        call(&mut dos, 3, 0, 0, 0).await;
        assert_eq!(state.cx(), 102);
    });
}

#[path = "mouse_dos/callbacks.rs"]
mod callbacks;
#[path = "mouse_dos/graphics_cursor.rs"]
mod graphics_cursor;
#[path = "mouse_dos/saved_state.rs"]
mod saved_state;
#[path = "mouse_dos/text_cursor.rs"]
mod text_cursor;
#[path = "mouse_dos/vbe_cursor.rs"]
mod vbe_cursor;
#[path = "mouse_dos/video_modes.rs"]
mod video_modes;
