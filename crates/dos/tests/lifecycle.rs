mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{FileAccess, LaunchError, LoadError, ServiceError};
use relocade_machine::{real_mode_interrupt, Hook};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{cell::Cell, io::Write, rc::Rc};

#[test]
fn session_memory_cpu_and_console_survive_program_termination() {
    pollster::block_on(async {
        let mut dos = relocade_dos::Dos::new().await.unwrap();
        assert!(!dos.has_active_process());
        let memory = dos.memory();
        let mut input = dos.stdin();
        let output = dos.stdout();
        let errors = dos.stderr();
        dos.mount_file("READY.TXT", b"before launch".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        input.write_all(b"AB").unwrap();
        memory.view().write(0x10500, b"retained").unwrap();
        let calls = Rc::new(Cell::new(0));
        memory.view().write(0x180, &[0, 8, 0, 0]).unwrap(); // INT 60h -> 0000:0800.
        memory
            .view()
            .write(0x800, &real_mode_interrupt::HOOK_STUB)
            .unwrap();
        let hook_calls = Rc::clone(&calls);
        dos.cpu()
            .install_hooks([Hook::new(0x800, move |call| {
                hook_calls.set(hook_calls.get() + 1);
                real_mode_interrupt::return_from_interrupt(call)
            })])
            .await
            .unwrap();
        let read_and_echo = [
            0xcd, 0x60, // The session's custom host hook.
            0xb4, 7, 0xcd, 0x21, // Read one character.
            0x88, 0xc2, 0xb4, 2, 0xcd, 0x21, // Write it to CON.
        ];
        for mz in [false, true] {
            let mut program = read_and_echo.to_vec();
            if mz {
                // The second program uses the console column left by the first.
                program.extend_from_slice(&[0xb2, 9, 0xb4, 2, 0xcd, 0x21]);
            }
            program.extend_from_slice(&[0xb8, 7, 0x4c, 0xcd, 0x21]); // Exit 7.
            let image = if mz {
                common::mz_image(&program)
            } else {
                program.to_vec()
            };
            if mz {
                dos.launch_mz(&image, Default::default()).unwrap();
            } else {
                dos.launch_com(&image, Default::default()).unwrap();
            }
            assert_eq!(dos.exit_status(), None);
            assert_eq!(dos.read_file("READY.TXT").unwrap(), b"before launch");
            assert_eq!(
                dos.cpu().run(100).unwrap(),
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(dos.exit_status(), Some(Ok(7)));
        }
        assert_eq!(calls.get(), 2); // The CPU retains its dispatch and hook registration.
        assert_eq!(common::output(output), b"AB      ");
        assert!(common::output(errors).is_empty());
        assert_eq!([0, 1].map(|i| dos.screen().cells[i].character), *b"AB");
        assert_eq!(dos.screen().cursor.column, 8);
        let mut retained = [0; 8];
        memory.view().read(0x10500, &mut retained).unwrap();
        assert_eq!(&retained, b"retained");

        // Rejected images leave the session's memory and exit status intact.
        memory.view().write(0x1fffe, &[0xaa, 0xbb]).unwrap();
        assert_eq!(
            common::load_error(dos.launch_com(&[], Default::default())),
            LoadError::EmptyImage
        );
        assert_eq!(
            common::load_error(dos.launch_mz(&[], Default::default())),
            LoadError::InvalidMzHeader
        );
        assert_eq!(memory.view().read_u16(0x1fffe).unwrap(), 0xbbaa);
        assert_eq!(dos.exit_status(), Some(Ok(7)));
        dos.launch_com(&[0xc3], Default::default()).unwrap();
        memory.view().read(0x10500, &mut retained).unwrap();
        assert_eq!(&retained, b"retained");
        assert_eq!(memory.view().read_u16(0x1fffe).unwrap(), 0);
        assert_eq!(
            dos.cpu().run(10).unwrap(),
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(dos.exit_status(), Some(Ok(0)));
    });
}

#[test]
fn termination_and_service_errors_release_process_files_and_searches() {
    pollster::block_on(async {
        for service_error in [false, true] {
            let mut dos = relocade_dos::Dos::new().await.unwrap();
            dos.mount('D', relocade_vfs::MemoryFileSystem::new())
                .unwrap();
            for name in ["D:DIR/A.BIN", "D:DIR/B.BIN"] {
                dos.mount_file(name, b"abc".to_vec(), FileAccess::ReadWrite)
                    .unwrap();
            }
            let memory = dos.memory();
            dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
            dos.stdin().write_all(b"old\rqueued").unwrap();
            assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
            service(&mut dos, 0x0e00, 0, 0, 3); // Select D:.
            success(&path_service(&mut dos, b"DIR", 0x3b00, 0));
            success(&path_service(&mut dos, b"*.BIN", 0x4e00, 0));
            let mut search = [0; 43];
            memory.view().read(0x10080, &mut search).unwrap();
            memory.view().write(0x10600, &search).unwrap();
            service(&mut dos, 0x1a00, 0, 0, 0x600);
            success(&path_service(&mut dos, b"A.BIN", 0x4301, 0));
            let handle = success(&path_service(&mut dos, b"A.BIN", 0x3d42, 0));
            success(&service(&mut dos, 0x4000, handle, 0, 0)); // Truncate and mark written.
            success(&service(&mut dos, 0x4600, handle, 1, 0)); // Redirect stdout.
            success(&service(&mut dos, 0x4401, 0, 0, 0x20)); // Raw stdin.
            success(&service(&mut dos, 0x3e00, 2, 0, 0));
            if service_error {
                {
                    let mut context = dos.cpu().context();
                    context.cpu.set_eip(0x100);
                    context.cpu.set_ax(0xfe00);
                }
                assert_eq!(
                    dos.cpu().run(10).unwrap(),
                    ExecutionExit::Host(HostExit::Return)
                );
                assert_eq!(
                    dos.exit_status(),
                    Some(Err(ServiceError::UnsupportedFunction(0xfe)))
                );
            } else {
                common::terminate(&mut dos);
            }
            let mut mcb = [0; 5];
            memory.view().read(0xfff0, &mut mcb).unwrap();
            assert_eq!(mcb, [b'Z', 0, 0, 0, 0x90]);

            dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
            assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 3);
            let attributes = path_service(&mut dos, b"A.BIN", 0x4300, 0);
            success(&attributes);
            assert_eq!(attributes.registers.ecx as u16, 0x20);
            assert_eq!(dos.read_file("D:DIR/A.BIN").unwrap(), b"");
            error(&service(&mut dos, 0x4400, handle, 0, 0), 6);
            for handle in 0..=2 {
                let info = service(&mut dos, 0x4400, handle, 0, 0);
                success(&info);
                assert_eq!(info.registers.edx as u16, 0x80c3);
            }
            // Console buffering belongs to the session; handle modes belong to the process.
            assert_eq!(success(&service(&mut dos, 0x3f00, 0, 20, 0x400)), 4);
            let mut remainder = [0; 4];
            memory.view().read(0x10400, &mut remainder).unwrap();
            assert_eq!(&remainder, b"ld\r\n");
            assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'q');
            assert_eq!(common::output(dos.stdout()), b"old\r\n");
            let dta = service(&mut dos, 0x2f00, 0, 0, 0);
            assert_eq!(dta.segments[Segment::Es].selector, 0x1000);
            assert_eq!(dta.registers.ebx as u16, 0x80);
            service(&mut dos, 0x1a00, 0, 0, 0x600);
            error(&service(&mut dos, 0x4f00, 0, 0, 0), 18);
        }
    });
}

#[test]
fn launch_requires_the_active_program_to_terminate() {
    pollster::block_on(async {
        let mut dos = relocade_dos::Dos::new().await.unwrap();
        let mut input = dos.stdin();
        let program = [
            0x90, // NOP: reach a budget boundary before the read.
            0xb4, 7, 0xcd, 0x21, // Await a character.
            0xb4, 0x4c, 0xcd, 0x21, // Terminate with that character's code.
        ];
        dos.launch_com(&program, Default::default()).unwrap();
        assert!(matches!(
            dos.launch_mz(&common::mz_image(&[0xc3]), Default::default()),
            Err(LaunchError::ProcessActive)
        ));
        assert_eq!(dos.cpu().run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert!(matches!(
            dos.launch_com(&[], Default::default()),
            Err(LaunchError::ProcessActive)
        ));
        assert_eq!(
            dos.cpu().run(100).unwrap(),
            ExecutionExit::Host(HostExit::Deferred)
        );
        assert!(dos.has_active_process());
        assert!(matches!(
            dos.launch_com(&[0xc3], Default::default()),
            Err(LaunchError::ProcessActive)
        ));
        input.write_all(b"X").unwrap();
        assert_eq!(
            dos.cpu().run_async(100).await.unwrap(),
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(dos.exit_status(), Some(Ok(b'X')));
        assert!(!dos.has_active_process());
        dos.launch_mz(
            &common::mz_image(&[0xb8, 9, 0x4c, 0xcd, 0x21]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(dos.exit_status(), None);
        assert_eq!(
            dos.cpu().run(100).unwrap(),
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(dos.exit_status(), Some(Ok(9)));
    });
}
