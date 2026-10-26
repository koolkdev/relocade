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
use relocade_dos::ProcessStatus;
use relocade_x86::{ExecutionExit, HostExit, Segment};
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
        let mut dos = common::dos(SERVICE_PROGRAM).await;

        {
            let mut machine = dos.cpu().context();
            let cpu = &mut machine.cpu;
            cpu.set_ax(0x077b);
            cpu.set_eflags(0x643);
        }
        assert_eq!(
            dos.cpu().run(10).unwrap(),
            ExecutionExit::Host(HostExit::Deferred)
        );
        assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        let suspended = dos.cpu().context().cpu.snapshot();
        assert_eq!(suspended.registers.eax as u16, 0x077b);
        assert_eq!(suspended.registers.esp as u16, 0xfff8);
        assert_eq!(suspended.instruction_count, 1); // INT retired once.
        let frame_address = suspended.segments.ss.base + 0xfff8;
        let mut frame = [0; 6];
        dos.cpu()
            .context()
            .memory
            .read(frame_address, &mut frame)
            .unwrap();
        assert_eq!(frame, [0x02, 0x01, 0x00, 0x10, 0x43, 0x06]); // IP, CS, FLAGS.

        assert_eq!(
            dos.cpu().run(10).unwrap(),
            ExecutionExit::Host(HostExit::Deferred)
        );
        assert_eq!(dos.cpu().context().cpu.snapshot(), suspended);
        let mut repeated_frame = [0; 6];
        dos.cpu()
            .context()
            .memory
            .read(frame_address, &mut repeated_frame)
            .unwrap();
        assert_eq!(repeated_frame, frame);

        dos.stdin().write_all(&[0, 0x48]).unwrap(); // Extended key prefix is data.
        assert_eq!(
            dos.cpu().run_async(10).await.unwrap(),
            ExecutionExit::BudgetExhausted
        );
        {
            let machine = dos.cpu().context();
            let cpu = &machine.cpu;
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
    });
}

#[test]
fn interactive_program_prints_prompt_waits_and_exits_after_input() {
    pollster::block_on(async {
        let mut dos = common::dos(&[
            0xba, 0x15, 1, // MOV DX,0115: prompt.
            0xb4, 9, 0xcd, 0x21, // Print the prompt.
            0xb4, 7, 0xcd, 0x21, // Wait for a raw character.
            0x88, 0xc2, // MOV DL,AL.
            0xb4, 6, 0xcd, 0x21, // Print that character.
            0xb4, 0x4c, 0xcd, 0x21, // Exit with AL.
            b'?', b' ', b'$',
        ])
        .await;

        let mut input = dos.stdin();
        let output = dos.stdout();
        let error_output = dos.stderr();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.cpu().run_async(100));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(common::output(output.clone()), b"? ");
            input.write_all(b"y").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(Ok(ExecutionExit::Host(HostExit::Return)))
            ));
        }
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(b'y'))
        );
        assert_eq!(common::output(output.clone()), b"y");
        assert!(common::output(error_output.clone()).is_empty());
    });
}

#[test]
fn blocking_read_follows_redirected_input_and_waits_at_file_end() {
    pollster::block_on(async {
        let mut dos = common::dos(SERVICE_PROGRAM).await;
        let mut disk = relocade_vfs::MemoryFileSystem::new();
        disk.replace_file("IN.TXT", vec![3]).unwrap();
        dos.mount('C', disk).unwrap();

        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(success(&service(&mut dos, 0x4500, 0, 0, 0)), 5);
        assert_eq!(success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0)), 6);
        success(&service(&mut dos, 0x4600, 6, 0, 0));
        let cpu = service(&mut dos, 0x0700, 0, 0, 0);
        assert_eq!(cpu.registers.eax as u16, 0x0703); // Ctrl-C is raw data.

        let before = dos.cpu().context().cpu.snapshot();
        dos.cpu().context().cpu.set_eip(0x100);
        assert_eq!(
            dos.cpu().run(10).unwrap(),
            ExecutionExit::Host(HostExit::Deferred)
        );
        let mut input = dos.stdin();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.cpu().run_async(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"Q").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // Abandon the EOF read and restore the caller before testing console input.
        dos.cpu().cancel_pending_call();
        dos.cpu().context().cpu.restore(&before).unwrap();
        assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 1); // Shared file cursor.
        success(&service(&mut dos, 0x4600, 5, 0, 0));
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074b
        );
    });
}

#[test]
fn stream_handles_remain_attached_to_their_original_environment() {
    pollster::block_on(async {
        let mut old = common::dos(SERVICE_PROGRAM).await;
        let mut old_input = old.stdin();
        let old_output = old.stdout();
        let old_error = old.stderr();
        old_input.write_all(b"old\rqueued").unwrap();
        assert_eq!(success(&service(&mut old, 0x3f00, 0, 1, 0x400)), 1);
        success(&service(&mut old, 0x4600, 2, 1, 0));
        service(&mut old, 0x0600, 0, 0, u16::from(b'E'));
        drop(old);

        let mut dos = common::dos(SERVICE_PROGRAM).await;
        old_input.write_all(b"later").unwrap();
        let cpu = service(&mut dos, 0x0600, 0, 0, 0xff);
        assert_ne!(cpu.flags.eflags() & 0x40, 0);
        assert!(common::output(dos.stdout()).is_empty());
        assert!(common::output(dos.stderr()).is_empty());
        dos.stdin().write_all(b"new\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 20, 0x400)), 5);
        let mut bytes = [0; 5];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"new\r\n");
        assert_eq!(common::output(dos.stdout()), b"new\r\n");
        assert_eq!(common::output(old_output), b"old\r\n");
        assert_eq!(common::output(old_error), b"E");
    });
}
