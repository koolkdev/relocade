use crate::common;
use relocade_bios::{KeyStroke, ShiftState};
use relocade_dos::{Dos, LaunchError};
use relocade_x86::{CpuState, ExecutionExit, HostExit};
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

// BIOS INT 16h/JMP $, followed by DOS INT 21h/JMP $.
const PROGRAM: &[u8] = &[0xcd, 0x16, 0xeb, 0xfe, 0xcd, 0x21, 0xeb, 0xfe];

fn invoke(dos: &mut Dos, ip: u32, ax: u16) -> CpuState {
    {
        let mut machine = dos.cpu().context();
        let cpu = &mut machine.cpu;
        cpu.set_eip(ip);
        cpu.set_ax(ax);
        cpu.set_bx(0xabcd);
        cpu.set_eflags(0x643);
    }
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::BudgetExhausted
    );
    let cpu = dos.cpu().context().cpu.snapshot();
    assert_eq!(cpu.registers.ebx as u16, 0xabcd);
    assert_eq!(cpu.registers.esp as u16, 0xfffe);
    cpu
}

#[test]
fn bios_peeks_reads_and_reports_shift_state_without_echo() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let keyboard = dos.keyboard();
        let cpu = invoke(&mut dos, 0x100, 0x017b);
        assert_eq!(cpu.flags.eflags(), 0x643);
        keyboard.push(KeyStroke {
            character: b'A',
            scan_code: 0x1e,
        });
        // Repeated peeks leave the key available and change only ZF.
        for _ in 0..2 {
            let cpu = invoke(&mut dos, 0x100, 0x0100);
            assert_eq!(cpu.registers.eax as u16, 0x1e41);
            assert_eq!(cpu.flags.eflags(), 0x603);
        }
        let cpu = invoke(&mut dos, 0x100, 0);
        assert_eq!(cpu.registers.eax as u16, 0x1e41);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);
        keyboard.set_shift_state(ShiftState {
            left_shift: true,
            control: true,
            caps_lock: true,
            ..Default::default()
        });
        let cpu = invoke(&mut dos, 0x100, 0x0200);
        assert_eq!(cpu.registers.eax as u16, 0x0246);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert!(common::output(dos.stdout()).is_empty());
    });
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn bios_wait_uses_the_existing_async_interrupt_return() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        dos.cpu().context().cpu.set_ax(0);
        dos.cpu().context().cpu.set_eflags(0x643);
        let keyboard = dos.keyboard();
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut run = pin!(dos.cpu().run(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            keyboard.push(KeyStroke {
                character: 0,
                scan_code: 0x48, // Up arrow.
            });
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(ExecutionExit::BudgetExhausted)
            ));
        }
        {
            let machine = dos.cpu().context();
            let cpu = &machine.cpu;
            assert_eq!(cpu.ax(), 0x4800);
            assert_eq!(cpu.eip(), 0x102);
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eflags(), 0x643);
        }
        // Dropping a waiter preserves the input registration and the BIOS read.
        {
            let mut machine = dos.cpu().context();
            machine.cpu.set_eip(0x100);
            machine.cpu.set_ax(0);
        }
        {
            let mut run = pin!(dos.cpu().run(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        keyboard.push(KeyStroke {
            character: b'X',
            scan_code: 0x2d,
        });
        assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
        assert_eq!(
            dos.launch_com(PROGRAM, Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
        assert_eq!(dos.cpu().context().cpu.ax(), 0x2d58);
        // Termination preserves queued BIOS keys, including their scan codes.
        keyboard.push(KeyStroke {
            character: b'Y',
            scan_code: 0x15,
        });
        {
            let mut execution = dos.cpu().context();
            execution.cpu.set_eip(0x104); // PROGRAM's DOS INT 21h entry.
            execution.cpu.set_ax(0x4c00);
        }
        assert_eq!(
            dos.cpu().run(10).await,
            ExecutionExit::Host(HostExit::Return)
        );
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let cpu = invoke(&mut dos, 0x100, 0x0100);
        assert_eq!(cpu.flags.eflags() & 0x40, 0);
        assert_eq!(cpu.registers.eax as u16, 0x1559);
        assert_eq!(invoke(&mut dos, 0x100, 0).registers.eax as u16, 0x1559);
        dos.cpu().context().cpu.set_eip(0x100);
        dos.cpu().context().cpu.set_ax(0);
        {
            let mut run = pin!(dos.cpu().run(10));
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // Dropping the environment releases the pending read's input registration.
        drop(dos);
        keyboard.push(KeyStroke {
            character: b'Z',
            scan_code: 0x2c,
        });
        assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);
    });
}

#[test]
fn dos_and_bios_consume_one_queue_with_a_dos_extended_key_remainder() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let keyboard = dos.keyboard();
        keyboard.push(KeyStroke {
            character: 0,
            scan_code: 0x4b, // Left arrow.
        });
        keyboard.push(KeyStroke {
            character: b'B',
            scan_code: 0x30,
        });
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, 0);
        // BIOS sees the next key; DOS retained the first key's second byte.
        assert_eq!(invoke(&mut dos, 0x100, 0).registers.eax as u16, 0x3042);
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, 0x4b);
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);

        // Byte-stream injection preserves literal zero and has no invented scan code.
        dos.stdin().write_all(&[0, b'C']).unwrap();
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, 0);
        assert_eq!(
            invoke(&mut dos, 0x100, 0).registers.eax as u16,
            u16::from(b'C')
        );

        keyboard.push(KeyStroke {
            character: b'D',
            scan_code: 0x20,
        });
        invoke(&mut dos, 0x104, 0x0cff); // DOS input flush also drains BIOS input.
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);
    });
}
