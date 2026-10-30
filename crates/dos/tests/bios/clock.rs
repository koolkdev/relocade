use crate::common;
use relocade_dos::ProcessStatus;
use relocade_x86::ExecutionExit;

async fn check_bios_clock() {
    let mut dos = common::dos().await;
    // INT 1Ah followed by JMP $ exposes the returned registers to the host.
    dos.launch_com(&[0xcd, 0x1a, 0xeb, 0xfe], Default::default())
        .unwrap();
    let mut initial = [0xff; 5];
    dos.cpu()
        .with(|call| call.memory.read(0x46c, &mut initial).unwrap());
    assert_eq!(initial, [0; 5]);
    dos.cpu()
        .with(|call| call.memory.write(0x46c, &[0x78, 0x56, 0x12, 0, 1]).unwrap());
    for rollover in [1, 0] {
        dos.cpu().with(|machine| {
            let cpu = &mut machine.cpu;
            cpu.set_eip(0x100);
            cpu.set_ax(0);
            cpu.set_bx(0xabcd);
            cpu.set_cx(0xffff);
            cpu.set_dx(0xffff);
            cpu.set_eflags(0x0a57); // OF, IF, ZF, AF, PF and CF.
        });
        assert_eq!(
            pollster::block_on(dos.cpu().run(10)),
            ExecutionExit::BudgetExhausted
        );
        let cpu = dos.cpu().with(|call| call.cpu.snapshot());
        assert_eq!(cpu.registers.eax as u16, rollover);
        assert_eq!(cpu.registers.ebx as u16, 0xabcd);
        assert_eq!(cpu.registers.ecx as u16, 0x12);
        assert_eq!(cpu.registers.edx as u16, 0x5678);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
        assert_eq!(cpu.flags.eflags(), 0x0a57); // Clock reads preserve flags.
        let mut data = [0; 5];
        dos.cpu()
            .with(|call| call.memory.read(0x46c, &mut data).unwrap());
        assert_eq!(data, [0x78, 0x56, 0x12, 0, 0]);
    }
    dos.cpu().with(|machine| {
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(0xff42);
        cpu.set_eflags(0x0a56); // Same flags, with carry clear.
    });
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::BudgetExhausted
    );
    let cpu = dos.cpu().with(|call| call.cpu.snapshot());
    assert_eq!(cpu.registers.eax as u16, 0x8642);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // Unsupported calls set only carry.
    assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
}

#[test]
fn bios_reads_ticks_and_consumes_midnight_rollover() {
    pollster::block_on(check_bios_clock());
}

#[test]
fn timer_irq_advances_the_bios_clock_during_bios_and_dos_input_waits() {
    use relocade_machine::{Clock, Runner};
    use relocade_x86::HostExit;
    use std::{
        future::{poll_fn, Future},
        pin::pin,
        task::Poll,
        time::Duration,
    };

    pollster::block_on(async {
        // Each row exercises a separately wired wait in the production DOS session.
        for (vector, function, expected_ax, expected_buffer, expected_output) in [
            (0x16, 0x0000, 0x1e41_u16, &[][..], &[][..]), // BIOS key.
            (0x21, 0x0100, 0x0141, &[][..], &b"A"[..]),   // DOS character + echo.
            (0x21, 0x0700, 0x0741, &[][..], &[][..]),     // DOS character.
            (0x21, 0x0a00, 0x0a00, &[4, 1, b'A', b'\r'][..], &b"A\r"[..]), // DOS line.
            (0x21, 0x3f00, 2, &b"A\r"[..], &b"A\r\n"[..]), // DOS console handle.
        ] {
            let (clock, timers) = relocade_machine::timer_service();
            std::thread::spawn(|| pollster::block_on(timers.run()));
            let runner = Runner::with_clock(clock.clone());
            let mut dos = relocade_dos::Dos::new(&runner).await.unwrap();
            dos.launch_com(
                &[
                    0xcd, vector, // Wait for input through the selected service.
                    0x2e, 0x8c, 0x1e, 0x1c, 3, // Record guest ISR's DS through CS.
                    0x0e, 0x1f, // Restore DS=CS for the result stores.
                    0x89, 0x1e, 0x16, 3, // Record the ISR's BX/CX/DX effects.
                    0x89, 0x0e, 0x18, 3, 0x89, 0x16, 0x1a, 3, 0xa3, 0x00,
                    0x03, // Save AX at DS:0300.
                    0xb8, 0x00, 0x00, 0xcd, 0x1a, // Read the BIOS clock.
                    0x89, 0x0e, 0x02, 0x03, // Save CX at DS:0302.
                    0x89, 0x16, 0x04, 0x03, // Save DX at DS:0304.
                    0xa2, 0x06, 0x03, // Save AL at DS:0306.
                    0xb8, 0x00, 0x4c, 0xcd, 0x21, // Exit to DOS.
                ],
                Default::default(),
            )
            .unwrap();
            dos.cpu().with(|call| {
                call.cpu.set_ax(function);
                call.cpu.set_bx(0); // Stdin for AH=3Fh.
                call.cpu.set_cx(2);
                call.cpu.set_dx(0x400);
                // Only the input wait enables live IF.
                call.cpu.set_eflags(0x443);
                // AH=0Ah line capacity.
                call.memory.write(0x10400, &[4, 0]).unwrap();
                // IRQ0 visits guest code and a nested DOS service before chaining to BIOS.
                call.memory.write(0x20, &[0, 6, 0, 0x10]).unwrap();
                call.memory
                    .write(
                        0x10600,
                        &[
                            0x50, 0x89, 0xe5, // PUSH AX; MOV BP,SP.
                            0x8b, 0x46, 2, 0x2e, 0xa3, 0x10, 3, // Record saved IRQ IP.
                            0x8b, 0x46, 4, 0x2e, 0xa3, 0x12, 3, // Record saved IRQ CS.
                            0x8b, 0x46, 6, 0x2e, 0xa3, 0x14, 3, // Record saved IRQ FLAGS.
                            0xb8, 0, 0x46, 0xbb, 2, 0, 0xb9, 1, 0, 0xcd,
                            0x21, // Redirect stdout to stderr.
                            0xb8, 0, 0x20, 0x8e, 0xd8, 0x58, // DS=2000; restore AX.
                            0xbb, 0xef, 0xbe, 0xb9, 0xad, 0xde, 0xba, 0x77,
                            0x77, // Change request registers.
                            0xea, 0x30, 0xff, 0,
                            0xf0, // Chain to BIOS IRQ0; its IRET reaches the resume entry.
                        ],
                    )
                    .unwrap();
            });
            let memory = dos.cpu().memory();
            memory
                .view()
                .write(0x46c, &[0xae, 0x00, 0x18, 0x00, 0])
                .unwrap();
            let keyboard = dos.keyboard();
            let exit = runner
                .run(async |runner| {
                    let mut execution = pin!(runner.run_cpu(dos.cpu()));
                    let mut timeout = pin!(clock.sleep(Duration::from_secs(5)));
                    let mut input_sent = false;
                    let mut saw_tick_before_midnight = false;
                    poll_fn(|cx| {
                        if let Poll::Ready(exit) = execution.as_mut().poll(cx) {
                            assert!(input_sent, "the input read completed after a timer IRQ");
                            return Poll::Ready(exit);
                        }
                        if memory.view().read_u32(0x46c).unwrap() == 0x18_00af {
                            saw_tick_before_midnight = true;
                        }
                        if !input_sent && memory.view().read_u8(0x470).unwrap() != 0 {
                            assert!(saw_tick_before_midnight);
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
                            input_sent = true;
                        }
                        assert!(
                            timeout.as_mut().poll(cx).is_pending(),
                            "timer IRQ did not arrive"
                        );
                        Poll::Pending
                    })
                    .await
                })
                .await;
            assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
            let mut result = [0; 7];
            memory.view().read(0x10300, &mut result).unwrap();
            let [al, ah] = expected_ax.to_le_bytes();
            assert_eq!(result, [al, ah, 0, 0, 0, 0, 1]);
            let mut buffer = vec![0; expected_buffer.len()];
            memory.view().read(0x10400, &mut buffer).unwrap();
            assert_eq!(buffer, expected_buffer);
            assert_eq!(memory.view().read_u32(0x46c).unwrap(), 0);
            assert_eq!(memory.view().read_u8(0x470).unwrap(), 0);
            let mut irq_and_registers = [0; 14];
            memory.view().read(0x10310, &mut irq_and_registers).unwrap();
            let resume_ip: u16 = if vector == 0x16 { 0xff11 } else { 0x0111 };
            let [resume_lo, resume_hi] = resume_ip.to_le_bytes();
            assert_eq!(
                irq_and_registers,
                [
                    // Saved service resume CS:IP and live IF.
                    resume_lo, resume_hi, 0, 0xf0, 0x43, 6,
                    // Guest effects survive resumption.
                    0xef, 0xbe, 0xad, 0xde, 0x77, 0x77, 0, 0x20,
                ]
            );
            assert_eq!(common::output(dos.stdout()), expected_output);
            assert!(
                common::output(dos.stderr()).is_empty(),
                "echo keeps its original destination"
            );
        }
    });
}
