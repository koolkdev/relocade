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
