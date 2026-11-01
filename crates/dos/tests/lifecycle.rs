mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{FileAccess, LaunchError, LoadError, ProcessStatus, ServiceError};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{
    future::Future,
    io::Write,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[test]
fn active_process_rejects_launches_without_changing_its_state() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA.BIN", b"abc".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d00, 0));
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 1, 0x400)), 1);
        service(&mut dos, 0x1a00, 0, 0, 0x600);
        service(&mut dos, 0x0200, 0, 0, u16::from(b'X'));
        let before_cpu = dos.cpu().context().cpu.snapshot();
        let mut before_memory = vec![0; 0x90010]; // Arena, PSP, program and environment.
        dos.cpu()
            .context()
            .memory
            .read(0xfff0, &mut before_memory)
            .unwrap();

        let mz = common::mz_image(&[0xb8, 0, 0x4c, 0xcd, 0x21]);
        for result in [
            dos.launch_com(&[0xc3], Default::default()),
            dos.launch_com(&[], Default::default()),
            dos.launch_mz(&mz, Default::default()),
            dos.launch_mz(b"invalid", Default::default()),
        ] {
            assert_eq!(result, Err(LaunchError::ProcessActive));
        }
        let mut after_memory = vec![0; before_memory.len()];
        dos.cpu()
            .context()
            .memory
            .read(0xfff0, &mut after_memory)
            .unwrap();
        assert_eq!(after_memory, before_memory);
        assert_eq!(dos.cpu().context().cpu.snapshot(), before_cpu);
        assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        assert_eq!(common::output(dos.stdout()), b"X");
        let cpu = service(&mut dos, 0x2f00, 0, 0, 0);
        assert_eq!(cpu.segments.es.selector, 0x1000);
        assert_eq!(cpu.registers.ebx as u16, 0x600);
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 1, 0x400)), 1);
        let mut byte = [0];
        dos.cpu().context().memory.read(0x10400, &mut byte).unwrap();
        assert_eq!(byte, [b'b']); // The open handle and its cursor survived.
    });
}

#[test]
fn normal_and_error_termination_release_process_resources() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        for (function, status) in [
            (0x4c07, Ok(7)),
            (0xfe00, Err(ServiceError::UnsupportedFunction(0xfe))),
        ] {
            dos.mount_file("A.DAT", b"abc".to_vec(), FileAccess::ReadWrite)
                .unwrap();
            dos.mount_file("B.DAT", vec![], FileAccess::ReadOnly)
                .unwrap();
            dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
            let process = dos.process().unwrap();
            success(&path_service(&mut dos, b"A.DAT", 0x4301, 0));
            let handle = success(&path_service(&mut dos, b"A.DAT", 0x3d42, 0));
            let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
            dos.cpu().context().memory.write(0x10400, b"Z").unwrap();
            assert_eq!(success(&service(&mut dos, 0x4000, handle, 1, 0x400)), 1);
            service(&mut dos, 0x1a00, 0, 0, 0x600);
            success(&path_service(&mut dos, b"*.DAT", 0x4e00, 0));
            let mut search = [0; 43];
            dos.cpu()
                .context()
                .memory
                .read(0x10600, &mut search)
                .unwrap();
            assert_eq!(&search[30..36], b"A.DAT\0");

            // Allocate another block so termination must release the whole arena.
            success(&service(&mut dos, 0x4a00, 0x1000, 0, 0));
            let allocated = success(&service(&mut dos, 0x4800, 0x10, 0, 0));
            let mut owner = [0; 2];
            dos.cpu()
                .context()
                .memory
                .read(u32::from(allocated) * 16 - 15, &mut owner)
                .unwrap();
            assert_eq!(owner, [0, 0x10]); // Bound process PSP.
            {
                let execution = dos.cpu().context();
                execution.cpu.set_eip(0x100);
                execution.cpu.set_ax(function);
            }
            assert_eq!(
                pollster::block_on(dos.cpu().run(10)),
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(status)
            );
            let mut mcb = [0; 5];
            dos.cpu().context().memory.read(0xfff0, &mut mcb).unwrap();
            assert_eq!(mcb, [b'Z', 0, 0, 0, 0x90]);
            assert_eq!(dos.read_file("A.DAT").unwrap(), b"Zbc");

            dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
            error(&service(&mut dos, 0x3e00, handle, 0, 0), 6);
            error(&service(&mut dos, 0x3e00, duplicate, 0, 0), 6);
            let cpu = service(&mut dos, 0x2f00, 0, 0, 0);
            assert_eq!(cpu.segments[Segment::Es].selector, 0x1000);
            assert_eq!(cpu.registers.ebx as u16, 0x80);
            dos.cpu().context().memory.write(0x10080, &search).unwrap();
            error(&service(&mut dos, 0x4f00, 0, 0, 0), 18);
            let cpu = path_service(&mut dos, b"A.DAT", 0x4300, 0);
            success(&cpu);
            assert_eq!(cpu.registers.ecx as u16, 0x20); // Close committed archive metadata.
            assert_eq!(process.status(), ProcessStatus::Exited(status));
            assert!(!Rc::ptr_eq(&process, &dos.process().unwrap()));
            common::terminate(&mut dos);
        }
    });
}

#[test]
fn retained_process_observes_input_wait_exit_and_successive_launches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        assert!(dos.process().is_none());
        assert_eq!(
            dos.launch_com(&[], Default::default()),
            Err(LaunchError::Load(LoadError::EmptyImage))
        );
        assert!(dos.process().is_none());
        dos.launch_com(
            &[
                0xb4, 7, 0xcd, 0x21, // Read a character.
                0xb4, 0x4c, 0xcd, 0x21, // Exit with that character.
            ],
            Default::default(),
        )
        .unwrap();
        let first = dos.process().unwrap();
        assert_eq!(first.psp_segment(), 0x1000);
        assert_eq!(first.status(), ProcessStatus::Active);
        assert!(Rc::ptr_eq(&first, &dos.process().unwrap()));
        assert_eq!(
            dos.launch_com(&[0xc3], Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert!(Rc::ptr_eq(&first, &dos.process().unwrap()));

        let mut input = dos.stdin();
        {
            let mut run = pin!(dos.cpu().run(100));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(first.status(), ProcessStatus::Active);
        }
        assert!(Rc::ptr_eq(&first, &dos.process().unwrap()));
        assert_eq!(
            dos.launch_com(&[0xc3], Default::default()),
            Err(LaunchError::ProcessActive)
        );
        dos.mount_file("WAIT.TXT", b"available".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        assert_eq!(dos.read_file("WAIT.TXT").unwrap(), b"available");
        {
            let mut run = pin!(dos.cpu().run(100));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(first.status(), ProcessStatus::Active);
            input.write_all(b"Q").unwrap();
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(ExecutionExit::Host(HostExit::Return))
            ));
            assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));
        }
        assert_eq!(
            dos.launch_mz(b"invalid", Default::default()),
            Err(LaunchError::Load(LoadError::InvalidMzHeader))
        );
        assert!(Rc::ptr_eq(&first, &dos.process().unwrap()));
        assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));

        dos.launch_mz(
            &common::mz_image(&[0xb4, 0xfe, 0xcd, 0x21]),
            Default::default(),
        )
        .unwrap();
        let second = dos.process().unwrap();
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(second.psp_segment(), first.psp_segment()); // Reused memory, new identity.
        assert_eq!(second.status(), ProcessStatus::Active);
        assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));
        assert_eq!(
            pollster::block_on(dos.cpu().run(100)),
            ExecutionExit::Host(HostExit::Return)
        );
        drop(dos);
        assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));
        assert_eq!(
            second.status(),
            ProcessStatus::Exited(Err(ServiceError::UnsupportedFunction(0xfe)))
        );
    });
}

#[test]
fn bios_input_can_complete_before_the_first_process_launch() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        {
            let mut execution = dos.cpu().context();
            execution
                .memory
                .write(0x7000, &[0xcd, 0x16, 0xeb, 0xfe]) // INT 16h; JMP $.
                .unwrap();
            execution.cpu.set_eip(0x7000);
            execution.cpu.set_sp(0x8000);
            execution.cpu.set_ax(0);
        }
        common::assert_pending(dos.cpu().run(10));
        assert!(dos.process().is_none());
        dos.keyboard().push(relocade_bios::KeyStroke {
            character: b'X',
            scan_code: 0x2d,
        });
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        assert_eq!(dos.cpu().context().cpu.ax(), 0x2d58);
        assert!(dos.process().is_none());
        dos.launch_com(&[0xc3], Default::default()).unwrap();
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
