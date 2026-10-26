use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    io::Write,
    pin::pin,
    task::{Context, Poll, Waker},
};

#[test]
fn console_handle_read_waits_for_enter_and_shares_the_remainder() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file(
            "IN.TXT",
            b"F\x1a\r\n".to_vec(),
            relocade_dos::FileAccess::ReadOnly,
        )
        .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        let output_file = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, output_file, 1, 0));
        dos.cpu()
            .context()
            .memory
            .write(0x10400, b"untouched")
            .unwrap();
        // A zero-length read returns immediately even with no console input.
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 0, 0x400)), 0);
        let mut unchanged = [0; 9];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut unchanged)
            .unwrap();
        assert_eq!(&unchanged, b"untouched");
        {
            let mut machine = dos.cpu().context();
            let cpu = &mut machine.cpu;
            cpu.set_eip(0x100);
            cpu.set_ax(0x3f00);
            cpu.set_bx(console);
            cpu.set_cx(2);
            cpu.set_dx(0x400);
            cpu.set_eflags(0x643);
        }
        let mut input = dos.stdin();
        let output = dos.stdout();
        {
            let mut run = pin!(dos.cpu().run_async(20));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"ab\x08C").unwrap();
            assert!(run.as_mut().poll(&mut cx).is_pending()); // CX bytes still need Enter.
            assert_eq!(common::output(output.clone()), b"ab\x08 \x08C");
            input.write_all(b"\r").unwrap();
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(Ok(ExecutionExit::BudgetExhausted))
            ));
        }
        {
            let machine = dos.cpu().context();
            machine.memory.read(0x10400, &mut unchanged).unwrap();
            assert_eq!(&unchanged, b"aCtouched");
            let cpu = &machine.cpu;
            assert_eq!(
                (cpu.ax(), cpu.bx(), cpu.cx(), cpu.dx()),
                (2, console, 2, 0x400)
            );
            assert_eq!(cpu.eflags(), 0x642);
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eip(), 0x102);
        }
        assert_eq!(common::output(output.clone()), b"\r\n");
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b""); // Echo belongs to CON.
        let screen = dos.screen();
        assert_eq!(screen.cells[0].character, b'a');
        assert_eq!(screen.cells[1].character, b'C'); // Backspace erased the previous b.
        assert_eq!(screen.cells[2].character, b' ');
        assert_eq!((screen.cursor.row, screen.cursor.column), (1, 0));
        common::error(&service(&mut dos, 0x3f00, 99, 0, 0x400), 6);
        assert_eq!(success(&service(&mut dos, 0x3f00, console, 0, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        let mut byte = [0];
        dos.cpu().context().memory.read(0x10400, &mut byte).unwrap();
        assert_eq!(byte, [b'\r']); // The original handle shares the duplicate's line.

        // Redirected file reads remain byte reads, independent of CON's pending LF.
        let file = success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        success(&service(&mut dos, 0x4600, file, 0, 0));
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0x400)), 4);
        let mut file_bytes = [0; 4];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut file_bytes)
            .unwrap();
        assert_eq!(&file_bytes, b"F\x1a\r\n");
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x3f00, console, 8, 0x400)), 1);
        dos.cpu().context().memory.read(0x10400, &mut byte).unwrap();
        assert_eq!(byte, [b'\n']);
        assert!(common::output(output.clone()).is_empty());

        input.write_all(b"one\rtwo\r").unwrap();
        for expected in [b"one\r\n", b"two\r\n"] {
            assert_eq!(success(&service(&mut dos, 0x3f00, console, 100, 0x400)), 5);
            let mut bytes = [0; 5];
            dos.cpu()
                .context()
                .memory
                .read(0x10400, &mut bytes)
                .unwrap();
            assert_eq!(&bytes, expected); // One line per read even when CX permits more.
        }
        assert_eq!(common::output(output), b"one\r\ntwo\r\n");
    });
}

#[test]
fn console_handle_read_bounds_the_line_and_reports_ctrl_z_eof() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.stdin().write_all(&[b'a'; 127]).unwrap();
        dos.stdin().write_all(b"X\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 500, 0x400)), 129);
        let mut bytes = [0; 129];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes[..127], &[b'a'; 127]);
        assert_eq!(&bytes[127..], b"\r\n");
        let output = common::output(dos.stdout());
        assert_eq!(&output[..127], &[b'a'; 127]);
        assert_eq!(&output[127..], b"\x07\r\n");

        dos.stdin().write_all(b"\x1a\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        let mut unchanged = [0; 129];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut unchanged)
            .unwrap();
        assert_eq!(unchanged, bytes);
        assert_eq!(common::output(dos.stdout()), b"^Z\r\n");
    });
}

#[test]
fn console_eof_is_shared_by_duplicates_and_cleared_by_handle_writes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&service(&mut dos, 0x4500, 0, 0, 0));
        let file = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));

        for output in [b"!".as_slice(), b"".as_slice()] {
            dos.stdin().write_all(b"\x1a\rRnext\r").unwrap();
            assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
            assert_eq!(common::output(dos.stdout()), b"^Z\r\n");
            let duplicate = success(&service(&mut dos, 0x4500, console, 0, 0));
            for handle in [0, console, duplicate] {
                assert_eq!(success(&service(&mut dos, 0x3f00, handle, 10, 0x400)), 0);
                assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x8083);
            }
            // EOF applies to handle reads; raw character input remains available.
            assert_eq!(
                service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
                0x0752
            );
            assert!(dos.launch_com(&[], Default::default()).is_err());
            assert_eq!(success(&service(&mut dos, 0x3f00, console, 10, 0x400)), 0);

            // Character output through an alias leaves EOF set, unlike AH=40h.
            success(&service(&mut dos, 0x4600, console, 1, 0));
            dos.cpu()
                .context()
                .memory
                .write(0x10420, b"message$")
                .unwrap();
            service(&mut dos, 0x0900, 0, 0, 0x420);
            service(&mut dos, 0x0600, 0, 0, u16::from(b'!'));
            assert_eq!(common::output(dos.stdout()), b"message!");
            assert_eq!(success(&service(&mut dos, 0x3f00, console, 10, 0x400)), 0);

            dos.cpu().context().memory.write(0x10400, b"!").unwrap();
            assert_eq!(success(&service(&mut dos, 0x4000, file, 1, 0x400)), 1);
            assert_eq!(success(&service(&mut dos, 0x3f00, duplicate, 10, 0x400)), 0);
            // Closing/replacing a handle slot does not reset the shared open record.
            success(&service(&mut dos, 0x3e00, 0, 0, 0));
            success(&service(&mut dos, 0x4600, duplicate, 0, 0));
            assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);

            assert_eq!(
                success(&service(
                    &mut dos,
                    0x4000,
                    console,
                    output.len() as u16,
                    0x400
                )),
                output.len() as u16
            );
            assert_eq!(common::output(dos.stdout()), output);
            for handle in [0, console, duplicate] {
                assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x80c3);
            }
            assert_eq!(success(&service(&mut dos, 0x3f00, duplicate, 10, 0x400)), 6);
            let mut bytes = [0; 6];
            dos.cpu()
                .context()
                .memory
                .read(0x10400, &mut bytes)
                .unwrap();
            assert_eq!(&bytes, b"next\r\n"); // Repeated EOF reads left queued input intact.
            assert_eq!(common::output(dos.stdout()), b"next\r\n");
            success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
        }

        dos.stdin().write_all(b"\x1a\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 0);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x80c3);
        dos.stdin().write_all(b"new\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 10, 0x400)), 5);
        assert_eq!(common::output(dos.stdout()), b"new\r\n");
    });
}

#[test]
fn program_reload_clears_the_console_line_remainder() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let mut input = dos.stdin();
        input.write_all(b"old\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        assert!(dos.launch_com(&[], Default::default()).is_err());
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        let mut byte = [0];
        dos.cpu().context().memory.read(0x10400, &mut byte).unwrap();
        assert_eq!(byte, [b'l']); // Failed loads preserve the current program.
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        input.write_all(b"new\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 20, 0x400)), 5);
        let mut bytes = [0; 5];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"new\r\n");
        assert_eq!(common::output(dos.stdout()), b"new\r\n");
    });
}
