#[path = "input/buffered.rs"]
mod buffered;
#[path = "input/characters.rs"]
mod characters;
mod common;
#[path = "input/flush.rs"]
mod flush;
#[path = "input/handles.rs"]
mod handles;
#[path = "input/raw.rs"]
mod raw;

use common::{path_service, service, success, SERVICE_PROGRAM};
use relocade_x86::{ExecutionExit, Segment};
use std::{
    future::Future,
    io::Write,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn blocking_read_resumes_the_existing_interrupt_frame() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        {
            let mut machine = dos.runtime().machine();
            let mut cpu = machine.cpu();
            cpu.set_ax(0x077b);
            cpu.set_eflags(0x643);
        }
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        assert_eq!(dos.exit_status(), None);
        let suspended = dos.runtime().machine().cpu().snapshot();
        assert_eq!(suspended.registers.eax as u16, 0x077b);
        assert_eq!(suspended.registers.esp as u16, 0xfff8);
        assert_eq!(suspended.instruction_count, 1); // INT retired once.
        let frame_address = suspended.segments.ss.base + 0xfff8;
        let mut frame = [0; 6];
        dos.runtime()
            .machine()
            .memory()
            .read(frame_address, &mut frame)
            .unwrap();
        assert_eq!(frame, [0x02, 0x01, 0x00, 0x10, 0x43, 0x06]); // IP, CS, FLAGS.

        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        assert_eq!(dos.runtime().machine().cpu().snapshot(), suspended);
        let mut repeated_frame = [0; 6];
        dos.runtime()
            .machine()
            .memory()
            .read(frame_address, &mut repeated_frame)
            .unwrap();
        assert_eq!(repeated_frame, frame);

        dos.stdin().write_all(&[0, 0x48]).unwrap(); // Extended key prefix is data.
        assert_eq!(
            dos.runtime().run_async(10).await.unwrap(),
            ExecutionExit::BudgetExhausted
        );
        {
            let mut machine = dos.runtime().machine();
            let cpu = machine.cpu();
            assert_eq!(cpu.ax(), 0x0700);
            assert_eq!(cpu.eip(), 0x102);
            assert_eq!(cpu.segment(Segment::Cs).selector, 0x1000);
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eflags(), 0x643);
        }
        let cpu = service(&mut dos, 0x0700, 0, 0, 0);
        assert_eq!(cpu.registers.eax as u16, 0x0748);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert!(common::output(dos.stdout()).is_empty()); // No echo.

        dos.runtime().machine().cpu().set_eip(0x100);
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        assert!(dos.load_com(&[], Default::default()).is_err());
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        let mut input = dos.stdin();
        input.write_all(b"discarded on load").unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let cpu = service(&mut dos, 0x0600, 0, 0, 0xff);
        assert_ne!(cpu.flags.eflags() & 0x40, 0);
        input.write_all(b"N").unwrap(); // Writers survive program replacement.
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074e
        );
        dos.runtime().machine().cpu().set_eip(0x100);
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        {
            let mut run = pin!(dos.runtime().run_async(10));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // A failed load preserves the interrupted call; a successful load
        // clears it together with the old program's CPU and stream state.
        let mz = common::mz_image(&[0xb8, 0, 0x4c, 0xcd, 0x21]);
        assert!(dos.load_mz(b"invalid", Default::default()).is_err());
        assert!(matches!(
            dos.runtime().run(10),
            Err(relocade_runtime::Error::CancelledHostCall)
        ));
        dos.load_mz(&mz, Default::default()).unwrap();
        assert_eq!(
            dos.runtime().run_async(10).await.unwrap(),
            ExecutionExit::HostReturn
        );
        assert_eq!(dos.exit_status(), Some(Ok(0)));
    });
}

#[test]
fn interactive_program_prints_prompt_waits_and_exits_after_input() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(
            &[
                0xba, 0x15, 1, // MOV DX,0115: prompt.
                0xb4, 9, 0xcd, 0x21, // Print the prompt.
                0xb4, 7, 0xcd, 0x21, // Wait for a raw character.
                0x88, 0xc2, // MOV DL,AL.
                0xb4, 6, 0xcd, 0x21, // Print that character.
                0xb4, 0x4c, 0xcd, 0x21, // Exit with AL.
                b'?', b' ', b'$',
            ],
            Default::default(),
        )
        .unwrap();
        let mut input = dos.stdin();
        let output = dos.stdout();
        let error_output = dos.stderr();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.runtime().run_async(100));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(common::output(output.clone()), b"? ");
            input.write_all(b"y").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(Ok(ExecutionExit::HostReturn))
            ));
        }
        assert_eq!(dos.exit_status(), Some(Ok(b'y')));
        assert_eq!(common::output(output.clone()), b"y");
        assert!(common::output(error_output.clone()).is_empty());
        // Readers also retain their identity across replacement and later output.
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        service(&mut dos, 0x0600, 0, 0, u16::from(b'O'));
        assert_eq!(common::output(output), b"O");
        success(&service(&mut dos, 0x4600, 2, 1, 0));
        service(&mut dos, 0x0600, 0, 0, u16::from(b'E'));
        assert_eq!(common::output(error_output), b"E");
    });
}

#[test]
fn blocking_read_follows_redirected_input_and_waits_at_file_end() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut disk = relocade_vfs::MemoryFileSystem::new();
        disk.replace_file("IN.TXT", vec![3]).unwrap();
        dos.mount('C', disk).unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(success(&service(&mut dos, 0x4500, 0, 0, 0)), 5);
        assert_eq!(success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0)), 6);
        success(&service(&mut dos, 0x4600, 6, 0, 0));
        let cpu = service(&mut dos, 0x0700, 0, 0, 0);
        assert_eq!(cpu.registers.eax as u16, 0x0703); // Ctrl-C is raw data.

        let before = dos.runtime().machine().cpu().snapshot();
        dos.runtime().machine().cpu().set_eip(0x100);
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        let mut input = dos.stdin();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.runtime().run_async(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"Q").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // Abandon the EOF read and restore the caller before testing console input.
        dos.runtime().cancel_pending_call();
        dos.runtime().machine().cpu().restore(&before).unwrap();
        assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 1); // Shared file cursor.
        success(&service(&mut dos, 0x4600, 5, 0, 0));
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074b
        );
    });
}
