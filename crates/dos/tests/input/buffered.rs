use super::{common, path_service, service, success, SERVICE_PROGRAM};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    io::Write,
    pin::pin,
    task::{Context, Poll, Waker},
};

#[test]
fn buffered_input_keeps_editing_across_waits_and_returns_through_iret() {
    pollster::block_on(async {
        let mut dos = common::dos(SERVICE_PROGRAM).await;

        dos.cpu()
            .unwrap()
            .memory()
            .view()
            .write(0x10300, &[5, 0, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xbb])
            .unwrap();
        {
            let mut machine = dos.cpu().unwrap().context();
            let mut cpu = machine.cpu();
            cpu.set_ax(0x0a7b);
            cpu.set_bx(0x1234);
            cpu.set_cx(0x5678);
            cpu.set_dx(0x300);
            cpu.set_eflags(0x643);
        }
        let mut input = dos.stdin();
        let output = dos.stdout();
        input.write_all(b"ab").unwrap();
        crate::common::assert_pending(dos.cpu().unwrap().run(10));
        assert_eq!(common::output(output.clone()), b"ab");
        let mut buffer = [0; 8];
        dos.cpu()
            .unwrap()
            .memory()
            .view()
            .read(0x10300, &mut buffer)
            .unwrap();
        assert_eq!(buffer, [5, 0, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xbb]);
        assert!(dos.cpu().unwrap().has_pending_call());
        crate::common::assert_pending(dos.cpu().unwrap().run(10));
        assert!(common::output(output.clone()).is_empty()); // No replayed echo.
        {
            let mut run = pin!(dos.cpu().unwrap().run(10));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"\x08\0").unwrap(); // Backspace; split extended key.
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(common::output(output.clone()), b"\x08 \x08");
            input.write_all(b"\x48c\rN").unwrap();
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(ExecutionExit::BudgetExhausted)
            ));
        }
        assert_eq!(common::output(output), b"c\r");
        {
            let mut machine = dos.cpu().unwrap().context();
            machine.memory().read(0x10300, &mut buffer).unwrap();
            assert_eq!(buffer, [5, 2, b'a', b'c', b'\r', 0xaa, 0xaa, 0xbb]);
            let cpu = machine.cpu();
            assert_eq!(
                (cpu.ax(), cpu.bx(), cpu.cx(), cpu.dx()),
                (0x0a7b, 0x1234, 0x5678, 0x300)
            );
            assert_eq!(cpu.eflags(), 0x643);
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eip(), 0x102);
        }
        assert_eq!(
            service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
            0x074e
        );
    });
}

#[test]
fn buffered_input_reserves_cr_space_and_handles_zero_capacity() {
    pollster::block_on(async {
        let mut dos = common::dos(SERVICE_PROGRAM).await;

        for (capacity, input, expected, echoed) in [
            (0u8, b"N".to_vec(), vec![], vec![]),
            (1, b"X\r".to_vec(), b"\r".to_vec(), b"\x07\r".to_vec()),
            (
                4,
                b"\x08abcX\x08D\r".to_vec(),
                b"abD\r".to_vec(),
                b"abc\x07\x08 \x08D\r".to_vec(),
            ),
            (
                255,
                [vec![b'z'; 254], b"X\r".to_vec()].concat(),
                [vec![b'z'; 254], b"\r".to_vec()].concat(),
                [vec![b'z'; 254], b"\x07\r".to_vec()].concat(),
            ),
        ] {
            let mut buffer = vec![0xaa; usize::from(capacity) + 3];
            buffer[0] = capacity;
            dos.cpu()
                .unwrap()
                .memory()
                .view()
                .write(0x10300, &buffer)
                .unwrap();
            dos.stdin().write_all(&input).unwrap();
            service(&mut dos, 0x0a00, 0, 0, 0x300); // Complete input finishes in this poll.
            let mut actual = vec![0; buffer.len()];
            dos.cpu()
                .unwrap()
                .memory()
                .view()
                .read(0x10300, &mut actual)
                .unwrap();
            if capacity != 0 {
                buffer[1] = (expected.len() - 1) as u8;
                buffer[2..2 + expected.len()].copy_from_slice(&expected);
            } else {
                assert_eq!(
                    service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u16,
                    0x074e
                );
            }
            assert_eq!(actual, buffer);
            assert_eq!(common::output(dos.stdout()), echoed);
        }
    });
}

#[test]
fn buffered_input_follows_stdin_and_stdout_redirection() {
    pollster::block_on(async {
        let mut dos = common::dos(SERVICE_PROGRAM).await;
        dos.mount_file(
            "IN.TXT",
            b"one\r\ntwo\r".to_vec(),
            relocade_dos::FileAccess::ReadOnly,
        )
        .unwrap();

        let input_handle = success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        success(&service(&mut dos, 0x4600, input_handle, 0, 0));
        let output_handle = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, output_handle, 1, 0));
        dos.stdin().write_all(b"K").unwrap();
        for expected in [b"one\r", b"two\r"] {
            dos.cpu()
                .unwrap()
                .memory()
                .view()
                .write(0x10400, &[8, 0])
                .unwrap();
            service(&mut dos, 0x0a00, 0, 0, 0x400);
            let mut buffer = [0; 6];
            dos.cpu()
                .unwrap()
                .memory()
                .view()
                .read(0x10400, &mut buffer)
                .unwrap();
            assert_eq!(&buffer[..2], &[8, 3]);
            assert_eq!(&buffer[2..], expected);
        }
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"one\rtwo\r");
        assert!(common::output(dos.stdout()).is_empty());
        assert_eq!(success(&service(&mut dos, 0x4201, input_handle, 0, 0)), 9);
        dos.cpu().unwrap().context().cpu().set_ax(0x0a00);
        dos.cpu().unwrap().context().cpu().set_dx(0x400);
        dos.cpu().unwrap().context().cpu().set_eip(0x100);
        common::assert_pending(dos.cpu().unwrap().run(10)); // EOF doesn't read K.
    });
}

#[test]
fn suspended_reads_keep_their_destination_and_buffer_across_run_waiters() {
    pollster::block_on(async {
        for (function, raw) in [(0x0a00, false), (0x3f00, false), (0x3f00, true)] {
            let mut dos = common::dos(SERVICE_PROGRAM).await;
            if raw {
                success(&service(&mut dos, 0x4401, 0, 0, 0xe3));
            }
            {
                let mut context = dos.cpu().unwrap().context();
                context
                    .memory()
                    .write(0x10300, &[6, 0, 0xaa, 0xaa, 0xaa, 0xaa])
                    .unwrap();
                context.memory().write(0x20500, &[0xcc; 6]).unwrap();
                context.cpu().set_eip(0x100);
                context.cpu().set_ax(function);
                context.cpu().set_bx(0);
                context.cpu().set_cx(3);
                context.cpu().set_dx(0x300);
            }
            dos.stdin().write_all(b"A").unwrap();
            common::assert_pending(dos.cpu().unwrap().run(10));
            // Shared memory remains accessible. A later capacity edit does not
            // replace the buffered read's already captured capacity.
            dos.cpu()
                .unwrap()
                .memory()
                .view()
                .write_u8(0x10300, 0)
                .unwrap();
            dos.stdin().write_all(b"B\r").unwrap();
            assert_eq!(
                dos.cpu().unwrap().run(1).await,
                ExecutionExit::BudgetExhausted
            );
            let mut context = dos.cpu().unwrap().context();
            let mut bytes = [0; 6];
            context.memory().read(0x10300, &mut bytes).unwrap();
            if function == 0x0a00 {
                assert_eq!(bytes, [0, 2, b'A', b'B', b'\r', 0xaa]);
            } else {
                assert_eq!(bytes, [b'A', b'B', b'\r', 0xaa, 0xaa, 0xaa]);
                assert_eq!(context.cpu().ax(), 3);
            }
            context.memory().read(0x20500, &mut bytes).unwrap();
            assert_eq!(bytes, [0xcc; 6]);
            assert_eq!(
                (context.cpu().bx(), context.cpu().cx(), context.cpu().dx()),
                (0, 3, 0x300)
            );
            assert_eq!(
                context.cpu().segment(relocade_x86::Segment::Ds).selector,
                0x1000
            );
        }
    });
}
