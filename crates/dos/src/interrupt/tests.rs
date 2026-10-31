use crate::Dos;
use relocade_machine::{timer_service, Runner};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    io::{Read, Write},
    pin::pin,
    task::{Context, Waker},
};

#[test]
fn pending_character_and_line_reads_retain_their_echo_destination() {
    pollster::block_on(async {
        for function in [0x01, 0x0a] {
            let (clock, _timers) = timer_service();
            let mut dos = Dos::new(&Runner::with_clock(clock)).await.unwrap();
            dos.launch_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
                .unwrap();
            {
                let mut call = dos.cpu().context();
                call.cpu.set_ah(function);
                call.cpu.set_dx(0x300);
                call.memory.write(0x10300, &[8, 0]).unwrap();
            }
            assert!(pin!(dos.cpu().run(10))
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            // Replacing the slot while waiting must not reroute this invocation's echo.
            dos.process()
                .unwrap()
                .files()
                .force_duplicate(&mut dos.state.filesystem(), 2, 1)
                .unwrap();
            dos.stdin()
                .write_all(if function == 0x01 { b"a" } else { b"ab\x08c\r" })
                .unwrap();
            assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            dos.stdout().read_to_end(&mut stdout).unwrap();
            dos.stderr().read_to_end(&mut stderr).unwrap();
            assert_eq!(
                stdout,
                if function == 0x01 {
                    &b"a"[..]
                } else {
                    &b"ab\x08 \x08c\r"[..]
                }
            );
            assert!(stderr.is_empty());
        }
    });
}

#[test]
fn pending_handle_read_keeps_its_source_and_partial_transfer() {
    pollster::block_on(async {
        let (clock, _timers) = timer_service();
        let mut dos = Dos::new(&Runner::with_clock(clock)).await.unwrap();
        dos.launch_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
            .unwrap();
        let process = dos.process().unwrap();
        // Raw console input with EOF clear, as selected by AX=4401h.
        process.files().set_device_information(0, 0x60).unwrap();
        {
            let mut call = dos.cpu().context();
            call.cpu.set_ax(0x3f00);
            call.cpu.set_bx(0);
            call.cpu.set_cx(2);
            call.cpu.set_dx(0x300);
            call.memory.write(0x10300, &[0xcc; 4]).unwrap();
        }
        dos.stdin().write_all(b"A").unwrap();
        assert!(pin!(dos.cpu().run(10))
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        // AUX would reject a new read. The pending read retains its console record.
        process
            .files()
            .force_duplicate(&mut dos.state.filesystem(), 3, 0)
            .unwrap();
        dos.stdin().write_all(b"BC").unwrap();
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        {
            let call = dos.cpu().context();
            assert_eq!(call.cpu.ax(), 2);
            assert!(!call.cpu.carry());
            let mut bytes = [0; 4];
            call.memory.read(0x10300, &mut bytes).unwrap();
            assert_eq!(bytes, [b'A', b'B', 0xcc, 0xcc]);
        }
        assert_eq!(dos.keyboard().try_read_character(), Some(b'C'));
        let mut output = Vec::new();
        dos.stdout().read_to_end(&mut output).unwrap();
        assert!(output.is_empty());
    });
}
