#[path = "../../machine/tests/support/clock.rs"]
mod clock;
mod common;

use clock::ManualClock;
use relocade_dos::Dos;
use relocade_pc_speaker::SPEAKER_CONTROL_PORT;
use std::{
    future::{poll_fn, Future},
    pin::{pin, Pin},
    time::Duration,
};

fn drive_until(mut execution: Pin<&mut impl Future>, condition: impl Fn() -> bool) {
    for _ in 0..20 {
        common::assert_pending(execution.as_mut());
        if condition() {
            return;
        }
    }
    panic!("execution did not reach the expected bell state");
}

#[test]
fn bios_bells_allow_timer_irqs_and_video_changes_during_output() {
    pollster::block_on(async {
        let clock = ManualClock::default();
        let (runner, _host) = common::runner(clock.clone());
        let (ready, mut prepared) = futures_channel::oneshot::channel();
        let execution = runner.run(std::future::pending::<()>(), async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(
                &[
                    0xb8, 7, 0x0e, 0xcd, 0x10, // Teletype BEL.
                    0x9c, 0x58, 0xa3, 4, 3, // Save returned flags.
                    0xc6, 6, 0, 3, 1, // Mark the first bell complete.
                    0xb8, 0x0d, 0, 0xcd, 0x10, // Graphics mode before string output.
                    0xb8, 3, 0x13, 0xbb, 7, 2, // AH=13h, inline attributes, page 2.
                    0xb9, 3, 0, 0xba, 2, 1, 0xbd, 0x10, 3, // Three cells at row 1, column 2.
                    0xcd, 0x10, 0xb8, 0, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            dos.cpu().cpu_state().set_eflags(2); // Only BIOS enables IRQs during the wait.
            let memory = dos.cpu().memory();
            memory
                .view()
                .write(0x10310, &[b'A', 0x2e, 7, 0x5f, b'B', 0x1e])
                .unwrap();
            let io = dos.cpu().context().io.clone();
            ready.send((memory.clone(), io)).ok().unwrap();
            assert_eq!(dos.run(runner).await, Ok(0));
            assert_eq!(memory.view().read_u16(0x10304).unwrap() & 0x200, 0);
            assert_eq!(memory.view().read_u16(0x454).unwrap(), 0x0104);
        });
        let mut execution = pin!(execution);
        let (memory, io) = poll_fn(|cx| {
            assert!(execution.as_mut().poll(cx).is_pending());
            Pin::new(&mut prepared).poll(cx)
        })
        .await
        .unwrap();
        drive_until(execution.as_mut(), || {
            io.read_port(SPEAKER_CONTROL_PORT, 1) & 3 == 3
        });
        clock.advance(Duration::from_millis(55));
        drive_until(execution.as_mut(), || {
            memory.view().read_u32(0x46c).unwrap() == 1
        });
        assert_eq!(memory.view().read_u8(0x10300).unwrap(), 0);
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        clock.advance(Duration::from_millis(100));
        drive_until(execution.as_mut(), || {
            memory.view().read_u8(0x10300).unwrap() == 1
        });
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        assert_eq!(memory.view().read_u8(0x449).unwrap(), 0x0d);
        // A guest IRQ switches back to text while the string is suspended at BEL.
        // The suffix must use the new layout and the string's retained position.
        let timer = memory.view().read_u32(8 * 4).unwrap();
        let mut handler = vec![0x50, 0xb8, 3, 0, 0xcd, 0x10, 0x58, 0xea];
        handler.extend_from_slice(&timer.to_le_bytes());
        memory.view().write(0x600, &handler).unwrap();
        memory.view().write_u32(8 * 4, 0x600).unwrap();
        clock.advance(Duration::from_millis(155));
        drive_until(execution.as_mut(), || {
            memory.view().read_u8(0x449).unwrap() == 3
        });
        clock.advance(Duration::from_millis(199));
        common::assert_pending(execution.as_mut());
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        clock.advance(Duration::from_millis(200));
        assert_eq!(execution.await, Some(()));
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0);
        assert_eq!(
            memory.view().with_io(&io).read_u16(0xba0a6).unwrap(),
            0x1e42
        );
    });
}

async fn manual_dos(clock: &ManualClock) -> Dos<ManualClock> {
    let (runner, _host) = common::runner(clock.clone());
    let mut dos = runner
        .run(std::future::pending::<()>(), async |runner| {
            Dos::new(runner).await.unwrap()
        })
        .await
        .unwrap();
    dos.cpu().interrupt_handle().take();
    dos.launch_com(common::SERVICE_PROGRAM, Default::default())
        .unwrap();
    dos
}

fn prepare_service(dos: &mut Dos<ManualClock>, ax: u16, bx: u16, cx: u16, dx: u16) {
    let state = dos.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(ax);
    state.set_bx(bx);
    state.set_cx(cx);
    state.set_dx(dx);
    state.set_eflags(2);
}

fn complete_service(dos: &mut Dos<ManualClock>) -> u16 {
    use std::task::{Context, Poll, Waker};
    {
        let mut run = pin!(common::run_until(dos.cpu(), 0x102));
        assert_eq!(
            run.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(relocade_x86::ExecutionExit::Interrupted)
        );
    }
    assert!(!dos.cpu().cpu_state().carry());
    dos.cpu().cpu_state().ax()
}

fn ready_service(dos: &mut Dos<ManualClock>, ax: u16, bx: u16, cx: u16, dx: u16) -> u16 {
    prepare_service(dos, ax, bx, cx, dx);
    complete_service(dos)
}

#[test]
fn dos_writes_and_echo_await_bells_and_dropping_the_session_disables_the_speaker() {
    use relocade_machine::Clock;
    use std::io::Write;

    pollster::block_on(async {
        let clock = ManualClock::default();
        let mut dos = manual_dos(&clock).await;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        for (ax, bx, cx, dx, data, input) in [
            (0x0200, 0, 0, 7, b"".as_slice(), b"".as_slice()),
            (0x0600, 0, 0, 7, b"".as_slice(), b"".as_slice()),
            (0x0900, 0, 0, 0x300, b"\x07$".as_slice(), b"".as_slice()),
            (0x4000, 1, 1, 0x300, b"\x07".as_slice(), b"".as_slice()),
            (0x0100, 0, 0, 0, b"".as_slice(), b"\x07".as_slice()),
            // Capacity 2 holds A plus CR; B triggers the line editor's overflow BEL.
            (0x0a00, 0, 0, 0x300, &[2, 0][..], b"AB\r".as_slice()),
        ] {
            memory.view().write(0x10300, data).unwrap();
            dos.stdin().write_all(input).unwrap();
            prepare_service(&mut dos, ax, bx, cx, dx);
            common::assert_pending(dos.cpu().run());
            assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3, "AH={ax:#x}");
            clock.advance(clock.now() + Duration::from_millis(100));
            complete_service(&mut dos);
            assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0, "AH={ax:#x}");
        }
        let mut line = [0; 4];
        memory.view().read(0x10300, &mut line).unwrap();
        assert_eq!(line, [2, 1, b'A', b'\r']);
        assert_eq!(common::output(dos.stdout()), b"\x07\x07\x07\x07\x07A\x07\r");
        prepare_service(&mut dos, 0x0200, 0, 0, 7);
        common::assert_pending(dos.cpu().run());
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        drop(dos); // Abandon the CPU's retained DOS/BIOS service future.
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0);
    });
}

#[test]
fn redirected_bell_bytes_do_not_ring_but_rust_console_output_still_uses_con() {
    use std::io::Write;

    pollster::block_on(async {
        let clock = ManualClock::default();
        let mut dos = manual_dos(&clock).await;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        for path in [b"BELL.TXT\0".as_slice(), b"NUL\0".as_slice()] {
            memory.view().write(0x10300, path).unwrap();
            let handle = ready_service(&mut dos, 0x3c00, 0, 0, 0x300);
            ready_service(&mut dos, 0x4600, handle, 1, 0); // Redirect stdout.
            memory.view().write(0x10300, b"\x07$").unwrap();
            ready_service(&mut dos, 0x0200, 0, 0, 7);
            ready_service(&mut dos, 0x0900, 0, 0, 0x300);
            assert_eq!(ready_service(&mut dos, 0x4000, 1, 1, 0x300), 1);
            dos.stdin().write_all(b"\x07").unwrap();
            assert_eq!(ready_service(&mut dos, 0x0100, 0, 0, 0) as u8, 7);
            ready_service(&mut dos, 0x3e00, handle, 0, 0);
            assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0);
        }
        assert_eq!(dos.read_file("BELL.TXT").unwrap(), [7; 4]);
        assert!(dos.read_file("NUL").is_none());
        assert!(common::output(dos.stdout()).is_empty());
        let console = dos.console();
        let mut write = pin!(console.write(b"\x07"));
        common::assert_pending(write.as_mut());
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        clock.advance(Duration::from_millis(100));
        write.await.unwrap();
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0);
    });
}
