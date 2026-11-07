use super::*;
use relocade_bios::KeyStroke;

#[test]
fn keyboard_stays_inhibited_during_the_break_callback_and_resumes_afterward() {
    use relocade_at_keyboard::{
        write_controller_command, DATA_PORT, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
    };
    use relocade_machine::{real_mode_interrupt::write_vector, FarPointer};
    const INTERFACE_INHIBITED: u8 = 1 << 4;

    with_hardware(async |runner, mut dos| {
        dos.launch_com(&[0xeb, 0], Default::default()).unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let exit = dos.cpu().exit_handle();
        dos.cpu()
            .install([relocade_machine::Hook::host(
                base + 0x102,
                async move || {
                    exit.request();
                },
            )])
            .await
            .unwrap();
        write_vector(
            &mut memory.view(),
            relocade_bios::interrupts::CONTROL_BREAK,
            FarPointer {
                segment,
                offset: 0x180,
            },
        );
        memory
            .view()
            .write(
                base + 0x180,
                &[
                    0x50, // Save AX.
                    0xb0, 0x20, 0xe6, 0x64, // Request the controller command byte.
                    0xe4, 0x64, 0xa8, 1, 0x74, 0xfa, // Wait for output.
                    0xe4, 0x60, 0x2e, 0xa2, 0, 3, // Save command byte at CS:0300h.
                    0x58, 0xcf, // Restore AX; IRET.
                ],
            )
            .unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        for byte in [0x1d, 0xe0, 0x46] {
            dos.cpu().cpu_state().set_eip(0x100);
            inject_scan(&io, byte).await;
            assert_eq!(
                runner.run_cpu(dos.cpu()).await,
                ExecutionExit::Host(relocade_x86::HostExit::Return)
            );
        }
        assert_ne!(
            memory.view().read_u8(base + 0x300).unwrap() & INTERFACE_INHIBITED,
            0
        );
        write_controller_command(&io, 0x20).await;
        io.wait_port(STATUS_COMMAND_PORT, 1, |status| {
            status & u32::from(OUTPUT_BUFFER_FULL) != 0
        })
        .await;
        assert_eq!(io.read_port(DATA_PORT, 1) as u8 & INTERFACE_INHIBITED, 0);
    });
}

#[test]
fn control_break_flushes_before_int1b_and_inserts_its_dummy_key_after_return() {
    with_hardware(async |_, mut dos| {
        dos.launch_com(&[0xeb, 0xfe], Default::default()).unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let keyboard = dos.keyboard();
        assert_eq!(memory.view().read_u8(0x471).unwrap(), 0);
        for _ in 0..15 {
            assert!(keyboard.store_key(KeyStroke {
                character: b'X',
                scan_code: 0x2d
            }));
        }
        memory
            .view()
            .write_u32(0x1b * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        memory
            .view()
            .write(
                base + 0x180,
                &[
                    0x9c, 0x58, 0x2e, 0xa3, 6, 3, // Save callback FLAGS.
                    0xa0, 0x71, 0, 0x2e, 0xa2, 0, 3, // Save BIOS break flag (DS=0040).
                    0xa1, 0x1a, 0, 0x2e, 0xa3, 2, 3, // Save buffer head.
                    0xa1, 0x1c, 0, 0x2e, 0xa3, 4, 3, // Save buffer tail.
                    0xb8, 0, 5, 0xb9, b'K', 0x25, 0xcd,
                    0x16, // Insert K through Rust INT 16h.
                    0xbb, 0xef, 0xbe, 0xb8, 0x22, 0x22, 0x8e, 0xd8, // Clobber BX and DS.
                    0xcf, // Return to the Rust BIOS IRQ1 handler.
                ],
            )
            .unwrap();
        let state = dos.cpu().cpu_state();
        state.set_ax(0x1234);
        state.set_bx(0x5678);
        state.set_cx(0x9abc);
        state.set_eflags(0x647); // Caller DF must be clear inside the IRQ callback.
        let before = state.snapshot();
        for byte in [0x1d, 0xe0, 0x46, 0xe0, 0xc6, 0x9d, 0x30] {
            inject_scan(&io, byte).await;
            guest_returns(&mut dos).await;
        }
        assert_eq!(memory.view().read_u8(base + 0x300).unwrap(), 0x80);
        assert_eq!(memory.view().read_u16(base + 0x306).unwrap() & 0x400, 0);
        assert_eq!(
            memory.view().read_u16(base + 0x302).unwrap(),
            memory.view().read_u16(base + 0x304).unwrap()
        );
        assert_eq!(memory.view().read_u8(0x471).unwrap(), 0x80);
        for key in [
            KeyStroke {
                character: b'K',
                scan_code: 0x25,
            },
            KeyStroke {
                character: 0,
                scan_code: 0,
            },
            KeyStroke {
                character: b'b',
                scan_code: 0x30,
            },
        ] {
            assert_eq!(keyboard.try_read_key(KeyMode::Enhanced), Some(key));
        }
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
        let after = state.snapshot();
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.segments, before.segments);
        assert_eq!(after.flags.eflags(), before.flags.eflags());
        assert!(!keyboard.shift_state().scroll_lock);
        assert!(!keyboard.shift_state().control);
    });
}

#[test]
fn default_break_callback_returns_and_wakes_a_bios_read() {
    with_hardware(async |_, mut dos| {
        dos.launch_com(&[0xcd, 0x16, 0xeb, 0xfe], Default::default())
            .unwrap();
        let io = dos.cpu().context().io.clone();
        dos.cpu().cpu_state().set_ax(0);
        common::assert_pending(common::run_until(dos.cpu(), 0x102));
        for byte in [0x1d, 0xe0] {
            inject_scan(&io, byte).await;
            pending_read_after_irq(&mut dos, &io, 0x102).await;
        }
        inject_scan(&io, 0x46).await;
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().cpu_state().ax(), 0);
        assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
        assert_eq!(dos.cpu().memory().view().read_u8(0x471).unwrap(), 0x80);
    });
}

#[test]
fn physical_control_break_reaches_dos_while_a_checked_read_is_pending() {
    for redirected in [false, true] {
        with_hardware(async move |_, mut dos| {
            let mut program = if redirected {
                dos.mount_file("EMPTY", Vec::new(), relocade_dos::FileAccess::ReadOnly)
                    .unwrap();
                vec![
                    0xb8, 0, 0x3d, 0xba, 0x80, 1, 0xcd, 0x21, // Open EMPTY.
                    0x89, 0xc3, 0x31, 0xc9, 0xb4, 0x46, 0xcd, 0x21, // Redirect stdin.
                    0xb4, 8, 0xcd, 0x21, 0xeb, 0xfe, // Checked read waits at EOF.
                ]
            } else {
                vec![0xcd, 0x21, 0xeb, 0xfe]
            };
            program.resize(0x80, 0);
            program.extend_from_slice(b"EMPTY\0");
            dos.launch_com(&program, Default::default()).unwrap();
            let after_read = if redirected { 0x114 } else { 0x102 };
            let io = dos.cpu().context().io.clone();
            dos.cpu().cpu_state().set_ax(0x0800);
            common::assert_pending(common::run_until(dos.cpu(), after_read));
            for byte in [0x1d, 0xe0] {
                inject_scan(&io, byte).await;
                pending_read_after_irq(&mut dos, &io, after_read).await;
            }
            inject_scan(&io, 0x46).await;
            assert_eq!(
                common::run_until(dos.cpu(), after_read).await,
                ExecutionExit::Host(relocade_x86::HostExit::Return)
            );
            assert_eq!(
                dos.process().unwrap().status(),
                relocade_dos::ProcessStatus::Exited(Ok(0))
            );
            assert_eq!(common::output(dos.stdout()), b"^C\r\n");
        });
    }
}
