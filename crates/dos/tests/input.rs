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
use relocade_dos::{LaunchError, ProcessStatus};
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
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.cpu().with(|machine| {
            let cpu = &mut machine.cpu;
            cpu.set_ax(0x077b);
            cpu.set_eflags(0x643);
        });
        crate::common::assert_pending(dos.cpu().run(10));
        assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        let memory = dos.cpu().memory();
        let frame_address = 0x1fff8;
        let mut frame = [0; 6];
        memory.view().read(frame_address, &mut frame).unwrap();
        assert_eq!(frame, [0x02, 0x01, 0x00, 0x10, 0x43, 0x06]); // IP, CS, FLAGS.

        crate::common::assert_pending(dos.cpu().run(10));
        let mut repeated_frame = [0; 6];
        memory
            .view()
            .read(frame_address, &mut repeated_frame)
            .unwrap();
        assert_eq!(repeated_frame, frame);

        dos.stdin().write_all(&[0, 0x48]).unwrap(); // Extended key prefix is data.
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        dos.cpu().with(|machine| {
            let cpu = &machine.cpu;
            assert_eq!(cpu.ax(), 0x0700);
            assert_eq!(cpu.eip(), 0x102);
            assert_eq!(cpu.segment(Segment::Cs).selector, 0x1000);
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eflags(), 0x643);
        });
        let cpu = service(&mut dos, 0x0700, 0, 0, 0);
        assert_eq!(cpu.registers.eax as u16, 0x0748);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert!(common::output(dos.stdout()).is_empty()); // No echo.

        dos.cpu().with(|call| call.cpu.set_eip(0x100));
        common::assert_pending(dos.cpu().run(10));
        let mut suspended_frame = [0; 6];
        memory
            .view()
            .read(frame_address, &mut suspended_frame)
            .unwrap();
        assert_eq!(
            dos.launch_com(&[], Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert_eq!(
            dos.launch_com(SERVICE_PROGRAM, Default::default()),
            Err(LaunchError::ProcessActive)
        );
        let mut after_rejection = [0; 6];
        memory
            .view()
            .read(frame_address, &mut after_rejection)
            .unwrap();
        assert_eq!(after_rejection, suspended_frame);
        common::assert_pending(dos.cpu().run(10));
        let mut input = dos.stdin();
        input.write_all(b"R").unwrap();
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        assert_eq!(dos.cpu().with(|call| call.cpu.ax()), 0x0752);
        input.write_all(b"Q").unwrap();
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let cpu = service(&mut dos, 0x0600, 0, 0, 0xff);
        assert_eq!(cpu.registers.eax as u16, 0x0651);
        assert_eq!(cpu.flags.eflags() & 0x40, 0);
        input.write_all(b"N").unwrap(); // Writers survive successive processes.
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074e
        );
        dos.cpu().with(|call| call.cpu.set_eip(0x100));
        common::assert_pending(dos.cpu().run(10));
        {
            let mut run = pin!(dos.cpu().run(10));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // Dropping a run waiter leaves the DOS process and its read active.
        // Rejected launches preserve that read for the next waiter.
        let mz = common::mz_image(&[0xb8, 0, 0x4c, 0xcd, 0x21]);
        assert_eq!(
            dos.launch_mz(b"invalid", Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert_eq!(
            dos.launch_mz(&mz, Default::default()),
            Err(LaunchError::ProcessActive)
        );
        input.write_all(b"T").unwrap();
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        assert_eq!(dos.cpu().with(|call| call.cpu.ax()), 0x0754);
        common::terminate(&mut dos);
        dos.launch_mz(&mz, Default::default()).unwrap();
        assert_eq!(
            dos.cpu().run(10).await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );
    });
}

#[test]
fn interactive_program_prints_prompt_waits_and_exits_after_input() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(
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
            let mut run = pin!(dos.cpu().run(100));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(common::output(output.clone()), b"? ");
            input.write_all(b"y").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(ExecutionExit::Host(HostExit::Return))
            ));
        }
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(b'y'))
        );
        assert_eq!(common::output(output.clone()), b"y");
        assert!(common::output(error_output.clone()).is_empty());
        // Readers also retain their identity across replacement and later output.
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
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
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(success(&service(&mut dos, 0x4500, 0, 0, 0)), 5);
        assert_eq!(success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0)), 6);
        success(&service(&mut dos, 0x4600, 6, 0, 0));
        let cpu = service(&mut dos, 0x0700, 0, 0, 0);
        assert_eq!(cpu.registers.eax as u16, 0x0703); // Ctrl-C is raw data.

        assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 1); // Shared file cursor.
        success(&service(&mut dos, 0x4600, 5, 0, 0));
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074b
        );
        success(&service(&mut dos, 0x4600, 6, 0, 0));
        dos.cpu().with(|call| call.cpu.set_ax(0x0700));
        dos.cpu().with(|call| call.cpu.set_eip(0x100));
        crate::common::assert_pending(dos.cpu().run(10));
        let mut input = dos.stdin();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.cpu().run(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"Q").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
    });
}
