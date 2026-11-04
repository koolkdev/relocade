use crate::common;
use relocade_dos::ProcessStatus;
use relocade_x86::ExecutionExit;

#[path = "../../../machine/tests/support/clock.rs"]
mod manual_clock;
use manual_clock::ManualClock;
use relocade_machine::ExecutionWorker;
use std::{
    future::Future,
    pin::{pin, Pin},
    time::{Duration, Instant},
};

// Literal channel-0 period at the BIOS divisor; production keeps this private.
const TICK: Duration = Duration::from_nanos(54_925_401);

fn drive_until(mut execution: Pin<&mut impl Future>, condition: impl Fn() -> bool) {
    for _ in 0..20 {
        common::assert_pending(execution.as_mut());
        if condition() {
            return;
        }
    }
    panic!("runner did not deliver the timer interrupt");
}

async fn check_bios_clock() {
    let mut dos = common::dos().await;
    // INT 1Ah followed by JMP $ exposes the returned registers to the host.
    dos.launch_com(&[0xcd, 0x1a, 0xeb, 0xfe], Default::default())
        .unwrap();
    let mut initial = [0xff; 5];
    dos.cpu()
        .context()
        .memory
        .read(0x46c, &mut initial)
        .unwrap();
    assert_eq!(initial, [0; 5]);
    dos.cpu()
        .context()
        .memory
        .write(0x46c, &[0x78, 0x56, 0x12, 0, 1])
        .unwrap();
    for rollover in [1, 0] {
        {
            let mut machine = dos.cpu().context();
            let cpu = &mut machine.cpu;
            cpu.set_eip(0x100);
            cpu.set_ax(0);
            cpu.set_bx(0xabcd);
            cpu.set_cx(0xffff);
            cpu.set_dx(0xffff);
            cpu.set_eflags(0x0a57); // OF, IF, ZF, AF, PF and CF.
        }
        assert_eq!(
            pollster::block_on(dos.cpu().run(10)),
            ExecutionExit::BudgetExhausted
        );
        let cpu = dos.cpu().context().cpu.snapshot();
        assert_eq!(cpu.registers.eax as u16, rollover);
        assert_eq!(cpu.registers.ebx as u16, 0xabcd);
        assert_eq!(cpu.registers.ecx as u16, 0x12);
        assert_eq!(cpu.registers.edx as u16, 0x5678);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
        assert_eq!(cpu.flags.eflags(), 0x0a57); // Clock reads preserve flags.
        let mut data = [0; 5];
        dos.cpu().context().memory.read(0x46c, &mut data).unwrap();
        assert_eq!(data, [0x78, 0x56, 0x12, 0, 0]);
    }
    {
        let mut machine = dos.cpu().context();
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(0xff42);
        cpu.set_eflags(0x0a56); // Same flags, with carry clear.
    }
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::BudgetExhausted
    );
    let cpu = dos.cpu().context().cpu.snapshot();
    assert_eq!(cpu.registers.eax as u16, 0x8642);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // Unsupported calls set only carry.
    assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
}

#[test]
fn bios_reads_ticks_and_consumes_midnight_rollover() {
    pollster::block_on(check_bios_clock());
}

#[test]
fn timer_advances_the_clock_during_guest_execution() {
    pollster::block_on(async {
        let clock = ManualClock::default();
        let (ready, shared_memory) = futures_channel::oneshot::channel();
        let worker_clock = clock.clone();
        let worker = ExecutionWorker::spawn(worker_clock, async move |runner| {
            let mut dos = relocade_dos::Dos::new(runner).await.unwrap();
            dos.launch_com(
                &[
                    0xc6, 0x06, 0, 3, 1, // Publish that guest execution has started.
                    0x31, 0xc0, 0x8e, 0xc0, // ES=0.
                    0x26, 0x83, 0x3e, 0x6c, 4, 0, 0x74, 0xf8, // Wait for BDA tick.
                    0xcd, 0x1a, // Read clock.
                    0x89, 0x16, 2, 3, // Save DX.
                    0xb8, 0, 0x4c, 0xcd, 0x21, // Exit.
                ],
                Default::default(),
            )
            .unwrap();
            assert!(ready.send(dos.cpu().memory()).is_ok());
            assert_eq!(dos.run(runner).await, Ok(0));
            dos.cpu().memory().view().read_u16(0x10302).unwrap()
        });
        let memory = shared_memory.await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while memory.view().read_u8(0x10300).unwrap() == 0 {
            assert!(Instant::now() < deadline, "guest execution did not start");
            std::thread::yield_now();
        }
        clock.advance(TICK);
        assert_eq!(worker.join().await.unwrap(), Some(1));
    });
}

#[test]
fn timer_irqs_chain_to_bios_during_bios_and_dos_input() {
    pollster::block_on(async {
        // Separate entry wiring: BIOS keyboard, DOS line editor, DOS console handle.
        for (vector, function, expected_ax, expected_buffer, expected_output) in [
            (0x16, 0x0000, 0x1e41_u16, &[][..], &[][..]),
            (0x21, 0x0a00, 0x0a00, &[4, 1, b'A', b'\r'][..], &b"A\r"[..]),
            (0x21, 0x3f00, 2, &b"A\r"[..], &b"A\r\n"[..]),
        ] {
            let clock = ManualClock::default();
            let (runner, _host) = common::runner(clock.clone());
            let mut dos = relocade_dos::Dos::new(&runner).await.unwrap();
            dos.launch_com(
                &[
                    0xcd, vector, 0x2e, 0x8c, 0x1e, 0x1c, 3, // Record ISR's DS through CS.
                    0x0e, 0x1f, // Restore DS=CS for result stores.
                    0x89, 0x1e, 0x16, 3, 0x89, 0x0e, 0x18, 3, 0x89, 0x16, 0x1a, 3, 0xa3, 0,
                    3, // Save AX.
                    0x31, 0xc0, 0xcd, 0x1a, // Read clock and consume rollover.
                    0x89, 0x0e, 2, 3, 0x89, 0x16, 4, 3, 0xa2, 6, 3, 0xb8, 0, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            let state = dos.cpu().cpu_state();
            state.set_ax(function);
            state.set_bx(0);
            state.set_cx(2);
            state.set_dx(0x400);
            state.set_eflags(0x443); // Caller IF clear; only the service enables it.
            let memory = dos.cpu().memory();
            let timer_vector = memory.view().read_u32(8 * 4).unwrap();
            let service_vector = memory.view().read_u32(u32::from(vector) * 4).unwrap();
            memory.view().write_u32(0x10320, timer_vector).unwrap();
            memory.view().write(8 * 4, &[0, 6, 0, 0x10]).unwrap();
            memory.view().write(0x10400, &[4, 0]).unwrap();
            memory
                .view()
                .write(
                    0x10600,
                    &[
                        0x50, 0x89, 0xe5, // PUSH AX; MOV BP,SP.
                        0x8b, 0x46, 2, 0x2e, 0xa3, 0x10, 3, // Saved IRQ IP.
                        0x8b, 0x46, 4, 0x2e, 0xa3, 0x12, 3, // Saved IRQ CS.
                        0x8b, 0x46, 6, 0x2e, 0xa3, 0x14, 3, // Saved IRQ FLAGS.
                        0xb8, 0, 0x46, 0xbb, 2, 0, 0xb9, 1, 0, 0xcd,
                        0x21, // stderr to stdout.
                        0xb8, 0, 0x20, 0x8e, 0xd8, 0x58, // DS=2000; restore AX.
                        0xbb, 0xef, 0xbe, 0xb9, 0xad, 0xde, 0xba, 0x77, 0x77, 0x2e, 0xff, 0x2e,
                        0x20, 3, // Far jump through saved BIOS vector.
                    ],
                )
                .unwrap();
            memory.view().write_u32(0x46c, 0x18_00ae).unwrap();
            let keyboard = dos.keyboard();
            {
                let execution = runner.run(std::future::pending::<()>(), async |runner| {
                    dos.run(runner).await
                });
                let mut execution = pin!(execution);
                common::assert_pending(execution.as_mut());
                assert!(state.interrupts_enabled());
                clock.advance(TICK);
                drive_until(execution.as_mut(), || {
                    memory.view().read_u32(0x46c).unwrap() == 0x18_00af
                });
                clock.advance(TICK * 2);
                drive_until(execution.as_mut(), || {
                    memory.view().read_u8(0x470).unwrap() == 1
                });
                assert_eq!(memory.view().read_u32(0x46c).unwrap(), 0);
                keyboard.push(relocade_bios::KeyStroke {
                    character: b'A',
                    scan_code: 0x1e,
                });
                if !expected_buffer.is_empty() {
                    keyboard.push(relocade_bios::KeyStroke {
                        character: b'\r',
                        scan_code: 0x1c,
                    });
                }
                assert_eq!(execution.await, Some(Ok(0)));
            }
            let mut result = [0; 7];
            memory.view().read(0x10300, &mut result).unwrap();
            let [al, ah] = expected_ax.to_le_bytes();
            assert_eq!(result, [al, ah, 0, 0, 0, 0, 1]);
            let mut buffer = vec![0; expected_buffer.len()];
            memory.view().read(0x10400, &mut buffer).unwrap();
            assert_eq!(buffer, expected_buffer);
            assert_eq!(memory.view().read_u8(0x470).unwrap(), 0);
            assert_eq!(
                memory.view().read_u32(0x10310).unwrap(),
                service_vector + 1,
                "IRQ return points at the suspended service's resume label"
            );
            assert_ne!(memory.view().read_u16(0x10314).unwrap() & 0x200, 0);
            let mut registers = [0; 8];
            memory.view().read(0x10316, &mut registers).unwrap();
            assert_eq!(registers, [0xef, 0xbe, 0xad, 0xde, 0x77, 0x77, 0, 0x20]);
            assert_eq!(common::output(dos.stdout()), expected_output);
            assert!(
                common::output(dos.stderr()).is_empty(),
                "echo keeps its destination"
            );
        }
    });
}
