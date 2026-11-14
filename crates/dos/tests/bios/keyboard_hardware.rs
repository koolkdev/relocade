use crate::common;
use relocade_bios::KeyMode;
use relocade_dos::Dos;
use relocade_frontend::input::InputEvent;
use relocade_machine::IoBus;
use relocade_x86::ExecutionExit;

#[path = "keyboard_hardware/control_break.rs"]
mod control_break;

// Freeze time so these tests isolate IRQ1 while the production runner drives
// controller firmware, keyboard firmware and guest execution.
type KeyboardDos = Dos;

fn with_hardware(check: impl AsyncFnOnce(KeyboardDos) + Send + 'static) {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let worker = prepared.spawn_with_clock(
        || relocade_clock_test_support::ManualClock::new().clock(),
        async move |runner| {
            let dos = Dos::new(runner).await.unwrap();
            check(dos).await;
        },
    );
    assert_eq!(pollster::block_on(worker.join()).unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}

async fn wait_status(io: &IoBus, flag: u8, set: bool) {
    for _ in 0..64 {
        if (io.read_port(relocade_ps2::STATUS_COMMAND_PORT, 1) & u32::from(flag) != 0) == set {
            return;
        }
        relocade_machine::yield_now().await;
    }
    panic!("keyboard firmware did not update status bit {flag:#x}");
}

async fn inject_scan(io: &IoBus, byte: u8) {
    use relocade_ps2::{DATA_PORT, INPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT};
    // Observe IBF for both writes, then wait for firmware to publish OBF/IRQ1.
    wait_status(io, INPUT_BUFFER_FULL, false).await;
    io.write_port(STATUS_COMMAND_PORT, 1, 0xd2);
    wait_status(io, INPUT_BUFFER_FULL, false).await;
    io.write_port(DATA_PORT, 1, u32::from(byte));
    wait_status(io, OUTPUT_BUFFER_FULL, true).await;
}

#[test]
fn cli_allows_guest_port_polling_without_bios_consuming_the_scan_byte() {
    with_hardware(async |mut dos| {
        dos.launch_com(
            &[
                0xfa, // CLI: firmware progress must not require guest IF.
                0xb0, 0xd2, 0xe6, 0x64, // Request controller output injection.
                0xe4, 0x64, 0xa8, 2, 0x75, 0xfa, // Poll IBF until clear.
                0xb0, 0x1e, 0xe6, 0x60, // Supply the scan byte.
                0xe4, 0x64, 0xa8, 1, 0x74, 0xfa, // Poll OBF until ready.
                0xa2, 0, 3, // Save status.
                0xe4, 0x60, 0xa2, 1, 3, // IN AL,60h; save scan byte.
                0xe4, 0x64, 0xa2, 2, 3, // IN AL,64h; save drained status.
                0xb8, 0, 0x4c, 0xcd, 0x21, // Exit.
            ],
            Default::default(),
        )
        .unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        assert_eq!(dos.run().await, Ok(0));
        assert_ne!(memory.view().read_u8(base + 0x300).unwrap() & 1, 0);
        assert_eq!(memory.view().read_u8(base + 0x301).unwrap(), 0x1e);
        assert_eq!(memory.view().read_u8(base + 0x302).unwrap() & 1, 0);
        assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
    });
}

async fn guest_returns(dos: &mut KeyboardDos) {
    assert_eq!(
        common::run_until(dos.cpu(), 0x100).await,
        ExecutionExit::Interrupted
    );
}

#[test]
fn replacing_irq1_owns_scan_consumption_and_guest_eoi_releases_the_next_irq() {
    with_hardware(async |mut dos| {
        dos.launch_com(&[0xeb, 0xfe], Default::default()).unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let old_mask = io.read_port(relocade_pic::MASK_PORT, 1);
        io.write_port(
            relocade_pic::MASK_PORT,
            1,
            old_mask | (1 << relocade_pic::KEYBOARD_IRQ),
        );
        memory
            .view()
            .write_u32(9 * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        memory
            .view()
            .write(
                base + 0x180,
                &[
                    0x50, 0x53, 0x1e, 0x0e, 0x1f, // Save AX/BX/DS; DS=CS.
                    0xe4, 0x60, // Consume the scan byte.
                    0x8b, 0x1e, 0, 3, // BX = scan count.
                    0x88, 0x87, 2, 3, // Save AL in scan log.
                    0xff, 0x06, 0, 3, // Increment count.
                    0x1f, 0x5b, 0x58, 0xcf, // Restore; IRET without EOI.
                ],
            )
            .unwrap();
        memory
            .view()
            .write(
                base + 0x110,
                &[
                    0xb0, 0x20, 0xe6, 0x20, // Non-specific EOI to the master PIC.
                    0xe9, 0xe9, 0xff, // JMP 0100h.
                ],
            )
            .unwrap();
        inject_scan(&io, 0x1e).await;
        guest_returns(&mut dos).await;
        assert_eq!(
            memory.view().read_u16(base + 0x300).unwrap(),
            0,
            "masked IRQ1 stays pending"
        );
        io.write_port(relocade_pic::MASK_PORT, 1, old_mask);
        guest_returns(&mut dos).await;
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 1);
        inject_scan(&io, 0x9e).await;
        guest_returns(&mut dos).await;
        assert_eq!(
            memory.view().read_u16(base + 0x300).unwrap(),
            1,
            "in-service IRQ1 blocks delivery"
        );
        dos.cpu().cpu_state().set_eip(0x110);
        guest_returns(&mut dos).await;
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 2);
        assert_eq!(memory.view().read_u16(base + 0x302).unwrap(), 0x9e1e);
        assert_eq!(
            dos.keyboard().peek_key(KeyMode::Enhanced),
            None,
            "replacement ISR bypasses BIOS translation"
        );
    });
}

#[test]
fn a_guest_irq1_hook_chains_to_bios_and_wakes_both_input_services() {
    with_hardware(async |mut dos| {
        dos.launch_com(
            &[0xcd, 0x16, 0xeb, 0xfe, 0xcd, 0x21, 0xeb, 0xfe],
            Default::default(),
        )
        .unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let original = memory.view().read_u32(9 * 4).unwrap();
        let mut handler = vec![0x2e, 0xff, 0x06, 0, 3, 0xea]; // INC CS:[0300h]; JMP FAR BIOS.
        handler.extend(original.to_le_bytes());
        memory.view().write(base + 0x180, &handler).unwrap();
        memory
            .view()
            .write_u32(9 * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        for (ip, ax, scan, expected) in [(0x100, 0, 0x1e, 0x1e61), (0x104, 0x0700, 0x30, 0x0762)] {
            dos.cpu().cpu_state().set_eip(ip);
            dos.cpu().cpu_state().set_ax(ax);
            dos.cpu().cpu_state().set_eflags(2);
            common::assert_pending(common::run_until(dos.cpu(), ip + 2));
            inject_scan(&io, scan).await;
            assert_eq!(
                common::run_until(dos.cpu(), ip + 2).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(dos.cpu().cpu_state().ax(), expected);
            assert!(
                !dos.cpu().cpu_state().interrupts_enabled(),
                "IRET restores caller IF"
            );
        }
        assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), 2);
        assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
        // INT16 reads a word inserted by guest software into the same ring.
        let tail = memory.view().read_u16(0x41c).unwrap();
        memory
            .view()
            .write_u16(0x400 + u32::from(tail), 0x2c5a)
            .unwrap();
        memory.view().write_u16(0x41c, tail + 2).unwrap();
        dos.cpu().cpu_state().set_eip(0x100);
        dos.cpu().cpu_state().set_ax(0);
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().cpu_state().ax(), 0x2c5a);
        assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
    });
}

#[test]
fn bios_translates_a_scan_already_read_by_a_guest_irq1_hook() {
    use relocade_bios::interrupts::KEYBOARD_IRQ;
    use relocade_machine::{
        real_mode_interrupt::{read_vector, write_vector},
        FarPointer,
    };

    with_hardware(async |mut dos| {
        dos.launch_com(&[0xeb, 0xfe], Default::default()).unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let io = dos.cpu().context().io.clone();
        let original = read_vector(&memory.view(), KEYBOARD_IRQ);
        let mut handler = vec![
            0x50, // Preserve AX across the hook.
            0xe4, 0x60, 0x2e, 0xa2, 0, 3, // Read and record the scan byte.
            0xe4, 0x64, 0x2e, 0xa2, 1, 3, // Record status after consuming it.
            0x58, 0xea, // Restore AX; JMP FAR saved BIOS handler.
        ];
        handler.extend(original.packed().to_le_bytes());
        memory.view().write(base + 0x180, &handler).unwrap();
        write_vector(
            &mut memory.view(),
            KEYBOARD_IRQ,
            FarPointer {
                segment,
                offset: 0x180,
            },
        );
        let state = dos.cpu().cpu_state();
        state.set_eflags(0x202);
        let stack = state.sp();
        let keyboard = dos.keyboard();

        for (scan, expected) in [
            (
                0x1e,
                Some(relocade_bios::KeyStroke {
                    character: b'a',
                    scan_code: 0x1e,
                }),
            ),
            (0x9e, None),
        ] {
            inject_scan(&io, scan).await;
            guest_returns(&mut dos).await;
            assert_eq!(memory.view().read_u8(base + 0x300).unwrap(), scan);
            assert_eq!(
                memory.view().read_u8(base + 0x301).unwrap() & relocade_ps2::OUTPUT_BUFFER_FULL,
                0,
                "BIOS is entered after the guest cleared OBF"
            );
            assert_eq!(keyboard.try_read_key(KeyMode::Enhanced), expected);
            assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
            assert_eq!(state.sp(), stack);
            assert!(state.interrupts_enabled());
        }
    });
}

#[test]
fn pending_irq1_does_not_consume_auxiliary_output() {
    use relocade_ps2::{
        AUXILIARY_OUTPUT, DATA_PORT, INPUT_BUFFER_FULL, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
    };

    with_hardware(async |mut dos| {
        dos.launch_com(&[0xeb, 0xfe], Default::default()).unwrap();
        dos.cpu().cpu_state().set_eflags(0x202);
        let io = dos.cpu().context().io.clone();
        inject_scan(&io, 0x30).await;
        // IRQ1 remains pending after a reader acknowledges the keyboard byte.
        assert_eq!(io.read_port(DATA_PORT, 1), 0x30);
        wait_status(&io, INPUT_BUFFER_FULL, false).await;
        io.write_port(STATUS_COMMAND_PORT, 1, 0xd3);
        wait_status(&io, INPUT_BUFFER_FULL, false).await;
        io.write_port(DATA_PORT, 1, 0x1e); // Valid scan code if the source is ignored.
        wait_status(&io, OUTPUT_BUFFER_FULL, true).await;
        guest_returns(&mut dos).await;
        assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
        assert_eq!(
            io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT),
            u32::from(OUTPUT_BUFFER_FULL | AUXILIARY_OUTPUT),
            "IRQ1 must leave auxiliary output available to its owner"
        );
        assert_eq!(io.read_port(DATA_PORT, 1), 0x1e);
    });
}

#[test]
fn reading_then_chaining_irq1_preserves_extended_key_make_and_break() {
    use relocade_frontend::keyboard::{
        ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, NativeKey,
    };
    use relocade_machine::Hook;

    with_hardware(async |mut dos| {
        // Wait for the requested number of IRQs before reaching the stop hook.
        let program = [
            0xa1, 0, 3, // AX = IRQ count.
            0x3b, 0x06, 0x20, 3, // CMP AX,[0320h].
            0x72, 0xf7, // JB 0100h.
            0xeb, 0, // Dispatch the stop hook.
        ];
        dos.launch_com(&program, Default::default()).unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        let original = memory.view().read_u32(9 * 4).unwrap();
        let mut handler = vec![
            0x50, 0x53, 0x1e, 0x0e, 0x1f, // Save AX/BX/DS; DS=CS.
            0xe4, 0x60, // Read the scan before chaining to BIOS.
            0x8b, 0x1e, 0, 3, // BX = IRQ count.
            0x88, 0x87, 2, 3, // Log AL at [0302h+BX].
            0xff, 0x06, 0, 3, // Increment IRQ count.
            0x88, 0xc3, 0x81, 0xe3, 0x7f, 0, 0xd1, 0xe3, // BX=(scan&7Fh)*2.
            0x25, 0x80, 0, 0xc1, 0xe8, 7, 0x35, 1, 0, // AX=make?1:0.
            0x89, 0x87, 0, 4, // Store held state in the 128-word table at 0400h.
            0xb8, 0x40, 0, 0x8e, 0xd8, // DS=BDA.
            0xa1, 0x1a, 0, 0xa3, 0x1c, 0, // Clear the BIOS ring.
            0x9c, 0x9a, // PUSHF; CALL FAR saved BIOS handler.
        ];
        handler.extend(original.to_le_bytes());
        handler.extend([
            0xb0, 0x20, 0xe6, 0x20, // EOI.
            0x1f, 0x5b, 0x58, 0xcf, // Restore DS/BX/AX; IRET.
        ]);
        memory.view().write(base + 0x180, &handler).unwrap();
        memory
            .view()
            .write_u32(9 * 4, (u32::from(segment) << 16) | 0x180)
            .unwrap();
        let exit = dos.cpu().exit_handle();
        dos.cpu()
            .install([Hook::host(
                (u32::from(segment) << 16) | (0x100 + program.len() as u32),
                async move || {
                    exit.request();
                },
            )])
            .await
            .unwrap();
        let state = dos.cpu().cpu_state();
        state.set_eflags(0x202);
        let stack = state.sp();

        for (transition, count, held) in [
            (ElementState::Pressed, 2, 1),
            (ElementState::Released, 4, 0),
        ] {
            memory.view().write_u16(base + 0x320, count).unwrap();
            state.set_eip(0x100);
            dos.keyboard_input()
                .unbounded_send(InputEvent::new(KeyboardEvent::Key {
                    physical_key: KeyCode::ArrowRight.into(),
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    location: KeyLocation::Standard,
                    text: None,
                    state: transition,
                    repeat: false,
                }))
                .unwrap();
            assert_eq!(
                dos.run().await,
                Err(relocade_dos::RunError::Execution(ExecutionExit::Host(
                    relocade_x86::HostExit::Return
                )))
            );
            assert_eq!(memory.view().read_u16(base + 0x300).unwrap(), count);
            assert_eq!(memory.view().read_u16(base + 0x49a).unwrap(), held);
            assert_eq!(state.sp(), stack, "both interrupt frames returned");
        }
        let mut scans = [0; 4];
        memory.view().read(base + 0x302, &mut scans).unwrap();
        assert_eq!(scans, [0xe0, 0x4d, 0xe0, 0xcd]);
    });
}
