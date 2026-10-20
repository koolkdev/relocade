use super::WakeCount;
use crate::common::{self, path_service, service, success, SERVICE_PROGRAM};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    io::Write,
    pin::pin,
    sync::{atomic::Ordering, Arc},
    task::{Context, Poll, Waker},
};

#[test]
fn raw_console_read_awaits_the_full_count_without_editing_or_echo() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let handle = success(&path_service(&mut dos, b"CON", 0x3d02, 0));
        success(&service(&mut dos, 0x4401, handle, 0, 0xe3));
        let mut input = dos.stdin();
        let output = dos.stdout();
        input.write_all(b"\x1a\x08").unwrap();
        {
            let mut machine = dos.runtime().machine();
            let mut cpu = machine.cpu();
            cpu.set_eip(0x100);
            cpu.set_ax(0x3f00);
            cpu.set_bx(handle);
            cpu.set_cx(5);
            cpu.set_dx(0x400);
        }
        assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::Deferred);
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.runtime().run_async(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            input.write_all(b"\0\r").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(run.as_mut().poll(&mut cx).is_pending()); // Four bytes, including Enter.
            assert!(common::output(output.clone()).is_empty());
            input.write_all(b"KZ").unwrap();
            assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(Ok(ExecutionExit::BudgetExhausted))
            ));
        }
        let cpu = dos.runtime().machine().cpu().snapshot();
        assert_eq!(success(&cpu), 5);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
        let mut bytes = [0; 5];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"\x1a\x08\0\rK");
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 1, 0x400)), 1);
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut bytes[..1])
            .unwrap();
        assert_eq!(bytes[0], b'Z');
        assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x80e3);
        assert!(common::output(output).is_empty());
    });
}

#[test]
fn raw_reads_preserve_cooked_remainders_and_respect_eof_and_segment_end() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.stdin().write_all(b"abc\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        assert_eq!(common::output(dos.stdout()), b"abc\r\n");
        success(&service(&mut dos, 0x4401, 0, 0, 0xe3));
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 0, 0x400)), 0);
        dos.stdin().write_all(b"R\x1a").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 2, 0x400)), 2);
        let mut bytes = [0; 4];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut bytes[..2])
            .unwrap();
        assert_eq!(&bytes[..2], b"R\x1a");
        success(&service(&mut dos, 0x4401, 0, 0, 0xc3));
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0x400)), 4);
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"bc\r\n");
        assert!(common::output(dos.stdout()).is_empty());

        success(&service(&mut dos, 0x4401, 0, 0, 0xa3)); // Raw, with EOF set.
        dos.stdin().write_all(b"XYZ").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 0);
        success(&service(&mut dos, 0x4401, 0, 0, 0xe3)); // Clear EOF.
        dos.runtime()
            .machine()
            .memory()
            .write(0x1ffff, b"??")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 3, 0xffff)), 1);
        dos.runtime()
            .machine()
            .memory()
            .read(0x1ffff, &mut bytes[..2])
            .unwrap();
        assert_eq!(&bytes[..2], b"X?");
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 2, 0x400)), 2);
        dos.runtime()
            .machine()
            .memory()
            .read(0x10400, &mut bytes[..2])
            .unwrap();
        assert_eq!(&bytes[..2], b"YZ");
        assert!(common::output(dos.stdout()).is_empty());
    });
}
