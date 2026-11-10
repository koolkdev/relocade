use crate::common;
use relocade_bios::{KeyMode, KeyStroke, ShiftState};
use relocade_dos::{Dos, LaunchError};
use relocade_frontend::input::InputEvent;
use relocade_x86::{CpuState, ExecutionExit, HostExit};
use std::{
    future::{poll_fn, Future},
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

#[path = "keyboard/enhanced.rs"]
mod enhanced;

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
        pollster::block_on(crate::common::run_until(dos.cpu(), ip + 2)),
        ExecutionExit::Interrupted
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
        assert_eq!(cpu.registers.eax as u16, 0x017b); // Empty peek preserves AX.
        let key = KeyStroke {
            character: b'A',
            scan_code: 0x1e,
        };
        assert!(keyboard.store_key(key));
        // Repeated peeks leave the key available and change only ZF.
        for _ in 0..2 {
            assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(key));
            let cpu = invoke(&mut dos, 0x100, 0x0100);
            assert_eq!(cpu.registers.eax as u16, 0x1e41);
            assert_eq!(cpu.flags.eflags(), 0x603);
        }
        let cpu = invoke(&mut dos, 0x100, 0);
        assert_eq!(cpu.registers.eax as u16, 0x1e41);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
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

#[test]
fn bios_store_key_uses_the_guest_ring_and_reports_full_without_modifying_it() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let memory = dos.cpu().memory();
        // A guest-relocated three-word ring has two usable entries. Start at its
        // last slot so the first BIOS store must wrap the tail.
        for (address, value) in [
            (0x480, 0x200),
            (0x482, 0x206),
            (0x41a, 0x204),
            (0x41c, 0x204),
        ] {
            memory.view().write_u16(address, value).unwrap();
        }
        for (word, address, tail) in [(0x1e61, 0x604, 0x200), (0x4800, 0x600, 0x202)] {
            dos.cpu().context().cpu.set_cx(word);
            dos.cpu().context().cpu.set_dx(0x1234);
            let cpu = invoke(&mut dos, 0x100, 0x05aa);
            assert_eq!(cpu.registers.eax as u16, 0x0500);
            assert_eq!(cpu.registers.ecx as u16, word);
            assert_eq!(cpu.registers.edx as u16, 0x1234);
            assert_eq!(cpu.flags.eflags(), 0x643);
            assert_eq!(memory.view().read_u16(address).unwrap(), word);
            assert_eq!(memory.view().read_u16(0x41c).unwrap(), tail);
            assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x204);
        }
        let mut before = [0; 6];
        memory.view().read(0x600, &mut before).unwrap();
        dos.cpu().context().cpu.set_cx(0x3062);
        let full = invoke(&mut dos, 0x100, 0x05aa);
        assert_eq!(full.registers.eax as u16, 0x0501);
        assert_eq!(full.registers.ecx as u16, 0x3062);
        assert_eq!(full.registers.edx as u16, 0x1234);
        assert_eq!(full.flags.eflags(), 0x643);
        let mut after = [0; 6];
        memory.view().read(0x600, &mut after).unwrap();
        assert_eq!(after, before);
        assert_eq!(memory.view().read_u16(0x41a).unwrap(), 0x204);
        assert_eq!(memory.view().read_u16(0x41c).unwrap(), 0x202);
        for word in [0x1e61, 0x4800] {
            assert_eq!(invoke(&mut dos, 0x100, 0).registers.eax as u16, word);
        }
        let empty = invoke(&mut dos, 0x100, 0x017b);
        assert_ne!(empty.flags.eflags() & 0x40, 0);
        assert_eq!(empty.registers.eax as u16, 0x017b);
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
            let mut run = pin!(crate::common::run_until(dos.cpu(), 0x102));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert!(keyboard.store_key(KeyStroke {
                character: 0,
                scan_code: 0x48, // Up arrow.
            }));
            assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                run.as_mut().poll(&mut cx),
                Poll::Ready(ExecutionExit::Interrupted)
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
            let machine = dos.cpu().context();
            machine.cpu.set_eip(0x100);
            machine.cpu.set_ax(0);
        }
        {
            let mut run = pin!(crate::common::run_until(dos.cpu(), 0x102));
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        assert!(keyboard.store_key(KeyStroke {
            character: b'X',
            scan_code: 0x2d,
        }));
        assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
        assert_eq!(
            dos.launch_com(PROGRAM, Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert_eq!(
            crate::common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().context().cpu.ax(), 0x2d58);
        // Termination preserves queued BIOS keys, including their scan codes.
        assert!(keyboard.store_key(KeyStroke {
            character: b'Y',
            scan_code: 0x15,
        }));
        {
            let execution = dos.cpu().context();
            execution.cpu.set_eip(0x104); // PROGRAM's DOS INT 21h entry.
            execution.cpu.set_ax(0x4c00);
        }
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let cpu = invoke(&mut dos, 0x100, 0x0100);
        assert_eq!(cpu.flags.eflags() & 0x40, 0);
        assert_eq!(cpu.registers.eax as u16, 0x1559);
        assert_eq!(invoke(&mut dos, 0x100, 0).registers.eax as u16, 0x1559);
        dos.cpu().context().cpu.set_eip(0x100);
        dos.cpu().context().cpu.set_ax(0);
        {
            let mut run = pin!(crate::common::run_until(dos.cpu(), 0x102));
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        // Dropping the environment releases the pending read's input registration.
        drop(dos);
        assert!(keyboard.store_key(KeyStroke {
            character: b'Z',
            scan_code: 0x2c,
        }));
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
        assert!(keyboard.store_key(KeyStroke {
            character: 0,
            scan_code: 0x4b, // Left arrow.
        }));
        assert!(keyboard.store_key(KeyStroke {
            character: b'B',
            scan_code: 0x30,
        }));
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, 0);
        // BIOS sees the next key; DOS retained the first key's second byte.
        assert_eq!(
            keyboard.peek_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character: b'B',
                scan_code: 0x30,
            })
        );
        assert_eq!(invoke(&mut dos, 0x100, 0x0100).registers.eax as u16, 0x3042);
        assert_eq!(invoke(&mut dos, 0x100, 0).registers.eax as u16, 0x3042);
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, 0x4b);
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);

        // BIOS sees the 0000h dummy word unchanged; CON filters it before returning C.
        dos.stdin().write_all(&[0, b'C']).unwrap();
        assert_eq!(invoke(&mut dos, 0x100, 0x0100).registers.eax as u16, 0);
        assert_eq!(invoke(&mut dos, 0x104, 0x0700).registers.eax as u8, b'C');
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);

        assert!(keyboard.store_key(KeyStroke {
            character: b'D',
            scan_code: 0x20,
        }));
        invoke(&mut dos, 0x104, 0x0cff); // DOS input flush also drains BIOS input.
        assert_ne!(invoke(&mut dos, 0x100, 0x0100).flags.eflags() & 0x40, 0);
    });
}

#[test]
fn host_events_reach_pending_bios_and_dos_reads_through_the_runner() {
    use relocade_frontend::keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent};
    use relocade_machine::timer_service;
    pollster::block_on(async {
        let (clock, timers) = timer_service();
        let timer_thread = std::thread::spawn(|| pollster::block_on(timers.run()));
        let (runner, _host, machine) = common::runner(clock.into()).await;
        let (ready, mut prepared) = futures_channel::oneshot::channel();
        let running = runner.run(std::future::pending::<()>(), async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            // BIOS reads AX into memory; DOS then reads another character and exits with it.
            dos.launch_com(
                &[
                    0xb4, 0, 0xcd, 0x16, 0xa3, 0, 3, 0xb4, 7, 0xcd, 0x21, 0xb4, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
            let memory = dos.cpu().memory();
            let input = dos.keyboard_input();
            ready.send(input).unwrap();
            assert_eq!(dos.run().await, Ok(b'b'));
            assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 0x1e61);
        });
        let mut running = pin!(running);
        let input = poll_fn(|cx| {
            assert!(running.as_mut().poll(cx).is_pending());
            std::pin::Pin::new(&mut prepared).poll(cx)
        })
        .await
        .unwrap();
        std::thread::spawn(move || {
            for (code, text) in [(KeyCode::KeyA, "ש"), (KeyCode::KeyB, "נ")] {
                input
                    .unbounded_send(InputEvent::new(KeyboardEvent::Key {
                        physical_key: code.into(),
                        logical_key: Key::Character(text.into()),
                        location: KeyLocation::Standard,
                        text: Some(text.into()),
                        state: ElementState::Pressed,
                        repeat: false,
                    }))
                    .unwrap();
            }
        })
        .join()
        .unwrap();
        assert_eq!(running.await, Some(()));
        timer_thread.join().unwrap();
        machine.shutdown().await.unwrap();
    });
}
