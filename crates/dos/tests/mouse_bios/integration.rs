use super::{
    callbacks::{deliver, inject, install},
    common, success, wait_status, with_mouse, write,
};
use relocade_bios::{KeyMode, KeyStroke};
use relocade_frontend::{
    keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent},
    mouse::{MouseButton, MouseEvent},
};
use relocade_x86::ExecutionExit;
use std::time::Duration;

#[test]
fn host_motion_reaches_bios_callback_through_sampled_ps2_and_irq12() {
    with_mouse(async |clock, mut dos| {
        install(&mut dos).await;
        let state = dos.cpu().cpu_state();
        state.set_interrupts_enabled(true);
        let mouse = dos.mouse_input();
        mouse
            .unbounded_send(MouseEvent::Button {
                button: MouseButton::Left,
                state: ElementState::Pressed,
            })
            .unwrap();
        mouse
            .unbounded_send(MouseEvent::Motion {
                delta_x: 7.0,
                delta_y: 3.0,
            })
            .unwrap();
        for _ in 0..4 {
            relocade_machine::yield_now().await;
        }
        clock.advance_to(relocade_machine::Timestamp::ZERO + Duration::from_millis(10));
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        for _ in 0..64 {
            common::run_until(dos.cpu(), 0x102).await;
            if memory.view().read_u16(base + 0x508).unwrap() == 1 {
                break;
            }
            relocade_machine::yield_now().await;
        }
        assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 1);
        let mut words = [0; 8];
        memory.view().read(base + 0x500, &mut words).unwrap();
        assert_eq!(words, [0, 0, 0xfd, 0, 7, 0, 0x29, 0]);
        assert_eq!(state.sp(), 0xfffe);
    });
}

#[test]
fn command_exchange_services_keyboard_and_retains_pending_auxiliary_irq() {
    with_mouse(async |_, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        // IRQ12 is already latched at the PIC when BIOS starts its command.
        inject(&io, 0x29).await;
        dos.keyboard_input()
            .unbounded_send(KeyboardEvent::Key {
                physical_key: KeyCode::KeyA.into(),
                logical_key: Key::Character("a".into()),
                location: KeyLocation::Standard,
                text: Some("a".into()),
                state: ElementState::Pressed,
                repeat: false,
            })
            .unwrap();
        let masks = (io.read_port(0x21, 1), io.read_port(0xa1, 1));
        let status = success(&mut dos, 6, 0).await;
        assert_eq!(status.registers.ebx as u16, 0x20);
        assert_eq!((io.read_port(0x21, 1), io.read_port(0xa1, 1)), masks);
        assert_eq!(
            dos.keyboard().try_read_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character: b'a',
                scan_code: 0x1e
            })
        );
        assert_eq!(
            memory.view().read_u16(base + 0x508).unwrap(),
            0,
            "command replies never become callback data"
        );
        dos.cpu().cpu_state().set_interrupts_enabled(true);
        deliver(&mut dos, [8, 1, 2]).await;
        assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 1);
    });
}

#[test]
fn caller_if_and_each_pic_mask_hold_mouse_bytes_until_delivery_is_allowed() {
    with_mouse(async |_, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let state = dos.cpu().cpu_state();
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        for (index, blocked) in [0, 0x21, 0xa1].into_iter().enumerate() {
            let masks = (io.read_port(0x21, 1), io.read_port(0xa1, 1));
            state.set_interrupts_enabled(blocked != 0);
            if blocked != 0 {
                io.write_port(
                    blocked,
                    1,
                    if blocked == 0x21 {
                        masks.0 | 4
                    } else {
                        masks.1 | 0x10
                    },
                );
            }
            inject(&io, 8).await;
            assert_eq!(
                common::run_until(dos.cpu(), 0x102).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(
                io.read_port(0x64, 1) & 0x21,
                0x21,
                "masked delivery leaves the byte unread"
            );
            assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), index as u16);
            io.write_port(0xa1, 1, masks.1);
            io.write_port(0x21, 1, masks.0);
            state.set_interrupts_enabled(true);
            common::run_until(dos.cpu(), 0x102).await;
            for byte in [1, 2] {
                inject(&io, byte).await;
                common::run_until(dos.cpu(), 0x102).await;
            }
            assert_eq!(
                memory.view().read_u16(base + 0x508).unwrap(),
                index as u16 + 1
            );
        }
    });
}

#[test]
fn callbacks_nest_bios_services_while_bios_and_dos_keyboard_reads_wait() {
    for vector in [0x16, 0x21] {
        with_mouse(async move |_, mut dos| {
            install(&mut dos).await;
            let io = dos.cpu().context().io.clone();
            let memory = dos.cpu().memory();
            let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
            memory
                .view()
                .write(base + 0x100, &[0xcd, vector, 0xeb, 0xfe])
                .unwrap();
            memory
                .view()
                .write(
                    base + 0x300,
                    &[
                        0xb8, 0, 5, 0xb9, b'K', 0x25, 0xcd,
                        0x16, // Insert a key through nested INT16.
                        0xeb, 0, // Stop at 030Ah before callback completion.
                        0xb8, 6, 0xc2, 0x31, 0xdb, 0xcd,
                        0x15, // Query the mouse from its callback.
                        0x2e, 0x88, 0x1e, 0x0a, 5, // Record status BL.
                        0x2e, 0xff, 0x06, 8, 5, 0xcb, // Count; RETF.
                    ],
                )
                .unwrap();
            let state = dos.cpu().cpu_state();
            state.set_eip(0x100);
            state.set_ax(if vector == 0x16 { 0 } else { 0x0700 });
            state.set_eflags(0x443);
            common::assert_pending(common::run_until(dos.cpu(), 0x102));
            assert!(state.interrupts_enabled());
            for byte in [8, 1] {
                inject(&io, byte).await;
                common::assert_pending(common::run_until(dos.cpu(), 0x102));
            }
            inject(&io, 2).await;
            assert_eq!(
                common::run_until(dos.cpu(), 0x30a).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(
                dos.keyboard().peek_key(KeyMode::Enhanced),
                Some(KeyStroke {
                    character: b'K',
                    scan_code: 0x25
                }),
                "outer read waits until callback returns"
            );
            assert_eq!(
                common::run_until(dos.cpu(), 0x102).await,
                ExecutionExit::Interrupted
            );
            assert_eq!(state.al(), b'K');
            assert_eq!(state.sp(), 0xfffe);
            assert!(!state.interrupts_enabled());
            assert_eq!(dos.keyboard().peek_key(KeyMode::Enhanced), None);
            assert_eq!(memory.view().read_u8(base + 0x50a).unwrap(), 0x20);
            assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 1);
        });
    }
}

#[test]
fn abandoning_a_mouse_callback_does_not_restore_suspended_bios_state() {
    with_mouse(async |_, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        memory
            .view()
            .write(base + 0x100, &[0xcd, 0x16, 0xeb, 0xfe])
            .unwrap();
        memory
            .view()
            .write(
                base + 0x300,
                &[0xfb, 0xb8, 0xad, 0xde, 0xba, 0x34, 0x12, 0xeb, 0xfe],
            )
            .unwrap();
        let state = dos.cpu().cpu_state();
        state.set_eip(0x100);
        state.set_ax(0);
        state.set_eflags(0x443);
        common::assert_pending(common::run_until(dos.cpu(), 0x102));
        for byte in [8, 1] {
            inject(&io, byte).await;
            common::assert_pending(common::run_until(dos.cpu(), 0x102));
        }
        inject(&io, 2).await;
        assert_eq!(
            common::run_until(dos.cpu(), 0x307).await,
            ExecutionExit::Interrupted
        );
        let before = state.snapshot();
        assert_eq!(state.ax(), 0xdead);
        assert!(state.interrupts_enabled());
        drop(dos);
        assert_eq!(state.snapshot(), before);
        for (port, active) in [(0x20, 4), (0xa0, 0x10)] {
            io.write_port(port, 1, 0x0b);
            assert_eq!(
                io.read_port(port, 1),
                active,
                "abandonment does not complete an IRQ"
            );
        }
    });
}

#[test]
fn a_blocked_keyboard_latch_times_out_mouse_commands_and_restores_pic_masks() {
    with_mouse(async |clock, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let master = io.read_port(0x21, 1);
        let slave = io.read_port(0xa1, 1);
        io.write_port(0x21, 1, master | 2); // IRQ1 cannot consume its byte.
        write(&io, 0x64, 0xd2).await;
        write(&io, 0x60, 0x1e).await;
        wait_status(&io, 0x21, 1).await;
        let state = dos.cpu().cpu_state();
        state.set_eip(0x100);
        state.set_ax(0xc204);
        state.set_eflags(0x443);
        for _ in 0..32 {
            common::assert_pending(common::run_until(dos.cpu(), 0x102));
            relocade_machine::yield_now().await;
        }
        assert_eq!(io.read_port(0x64, 1) & 0x23, 1);
        clock.advance_to(relocade_machine::Timestamp::ZERO + Duration::from_millis(100));
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(state.ax(), 0x0304);
        assert_eq!(state.eflags(), 0x443);
        assert_eq!(
            (io.read_port(0x21, 1), io.read_port(0xa1, 1)),
            (master | 2, slave)
        );
        io.write_port(0x21, 1, master);
        success(&mut dos, 4, 0).await;
        assert_eq!(
            dos.keyboard().try_read_key(KeyMode::Enhanced),
            Some(KeyStroke {
                character: b'a',
                scan_code: 0x1e
            })
        );
    });
}
