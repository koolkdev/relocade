mod common;
#[path = "lifecycle/vectors.rs"]
mod vectors;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_bios::KeyMode;
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
fn successive_com_and_mz_launches_preserve_session_retirement_and_time() {
    use relocade_machine::ExecutionWorker;
    use std::num::NonZeroU64;

    pollster::block_on(async {
        let worker = ExecutionWorker::spawn_with_guest_time(
            || relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()),
            async |runner| {
                let mut dos = relocade_dos::Dos::new(runner).await.unwrap();
                let program = [0xb8, 7, 0x4c, 0xcd, 0x21];
                for mz in [false, true, false] {
                    let count = dos.cpu().cpu_state().instruction_count();
                    let time = runner.clock().now();
                    if mz {
                        dos.launch_mz(&common::mz_image(&program), Default::default())
                            .unwrap();
                    } else {
                        dos.launch_com(&program, Default::default()).unwrap();
                    }
                    assert_eq!(dos.cpu().cpu_state().instruction_count(), count);
                    assert_eq!(runner.clock().now(), time);
                    assert_eq!(dos.run(runner).await, Ok(7));
                    assert!(dos.cpu().cpu_state().instruction_count() > count);
                    assert!(runner.clock().now() > time);
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(worker.join().await.unwrap(), Some(()));
    });
}

#[test]
fn installing_dos_preserves_the_existing_pc_and_bios() {
    use relocade_machine::timer_service;

    pollster::block_on(async {
        let (clock, _timers) = timer_service();
        let (runner, _host) = common::runner(clock.clock());
        runner
            .run(
                futures_timer::Delay::new(std::time::Duration::from_secs(30)),
                async |runner| {
                    let mut pc = relocade_pc::Pc::new(runner).await.unwrap();
                    let vga = pc.vga();
                    let speaker = pc.speaker();
                    let mut keyboard = pc.keyboard();
                    // Preserve a full BIOS ring when attaching DOS to the PC.
                    keyboard.write_all(b"abcdefghijklmno").unwrap();
                    let memory = pc.cpu().memory();
                    let io = pc.cpu().context().io.clone();
                    io.write_port(0x21, 1, 0xff); // Keep the programmed mask and clock stable.
                    memory.view().write_u32(0x46c, 12345).unwrap();
                    pc.cpu()
                        .context()
                        .memory
                        .write_u16(0xb8000, 0x1f58)
                        .unwrap();
                    let vectors = [0x08, 0x09, 0x10, 0x16, 0x1a]
                        .map(|vector| memory.view().read_u32(vector * 4).unwrap());
                    let cpu = pc.cpu().cpu_state().snapshot();

                    let mut dos = relocade_dos::Dos::from_pc(pc).await.unwrap();
                    assert!(Rc::ptr_eq(&vga, &dos.vga()));
                    assert!(Rc::ptr_eq(&speaker, &dos.speaker()));
                    assert_eq!(dos.cpu().cpu_state().snapshot(), cpu);
                    assert_eq!(memory.view().read_u32(0x46c).unwrap(), 12345);
                    assert_eq!(io.read_port(0x21, 1), 0xff);
                    assert_eq!(
                        dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
                        0x1f58
                    );
                    assert_eq!(
                        [0x08, 0x09, 0x10, 0x16, 0x1a]
                            .map(|vector| memory.view().read_u32(vector * 4).unwrap()),
                        vectors,
                    );
                    assert_eq!(memory.view().read_u8(0xfff0).unwrap(), b'Z');

                    dos.launch_com(
                        &[
                            0x31, 0xc0, 0xcd, 0x16, 0xa3, 0, 3, // Read 'a' through BIOS.
                            0xb4, 7, 0xcd, 0x21, 0xa2, 2, 3, // Read 'b' through DOS.
                            0xb4, 2, 0xb2, b'Y', 0xcd,
                            0x21, // Console output through shared VGA MMIO.
                            0xb8, 5, 0x4c, 0xcd, 0x21,
                        ],
                        Default::default(),
                    )
                    .unwrap();
                    assert_eq!(dos.run(runner).await, Ok(5));
                    assert_eq!(memory.view().read_u16(0x10300).unwrap(), u16::from(b'a'));
                    assert_eq!(memory.view().read_u8(0x10302).unwrap(), b'b');
                    assert_eq!(dos.cpu().context().memory.read_u8(0xb8000).unwrap(), b'Y');
                    let mut remaining = Vec::new();
                    while let Some(key) = keyboard.try_read_key(KeyMode::Enhanced) {
                        remaining.push(key.character);
                    }
                    assert_eq!(remaining, b"cdefghijklmno");
                },
            )
            .await
            .expect("DOS guest timed out");
    });
}

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
            (0x2a00, Err(ServiceError::UnsupportedFunction(0x2a))),
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
                pollster::block_on(dos.cpu().run()),
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
            let mut run = pin!(dos.cpu().run());
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
            let mut run = pin!(dos.cpu().run());
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
            &common::mz_image(&[0xb4, 0x2a, 0xcd, 0x21]),
            Default::default(),
        )
        .unwrap();
        let second = dos.process().unwrap();
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(second.psp_segment(), first.psp_segment()); // Reused memory, new identity.
        assert_eq!(second.status(), ProcessStatus::Active);
        assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));
        assert_eq!(
            pollster::block_on(dos.cpu().run()),
            ExecutionExit::Host(HostExit::Return)
        );
        drop(dos);
        assert_eq!(first.status(), ProcessStatus::Exited(Ok(b'Q')));
        assert_eq!(
            second.status(),
            ProcessStatus::Exited(Err(ServiceError::UnsupportedFunction(0x2a)))
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
        common::assert_pending(dos.cpu().run());
        assert!(dos.process().is_none());
        assert!(dos.keyboard().store_key(relocade_bios::KeyStroke {
            character: b'X',
            scan_code: 0x2d,
        }));
        assert_eq!(
            crate::common::run_until(dos.cpu(), 0x7002).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().context().cpu.ax(), 0x2d58);
        assert!(dos.process().is_none());
        dos.launch_com(&[0xc3], Default::default()).unwrap();
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );
    });
}
