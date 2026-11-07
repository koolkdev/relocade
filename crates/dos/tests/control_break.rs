mod common;

use common::{service, SERVICE_PROGRAM};
use relocade_dos::{Dos, ProcessStatus};
use relocade_machine::{real_mode_interrupt::Interrupts, registers as reg, FarPointer};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::io::Write;

async fn session() -> Dos {
    let mut dos = common::dos().await;
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos
}

fn install(dos: &mut Dos, handler: &[u8]) {
    dos.cpu().memory().view().write(0x10180, handler).unwrap();
    service(dos, 0x2523, 0, 0, 0x180);
}

fn start(dos: &mut Dos, function: u16) {
    let state = dos.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(function);
    state.set_bx(0);
    state.set_cx(1);
    state.set_dx(0x400);
}

#[test]
fn break_setting_is_queryable_exchangeable_and_persistent() {
    pollster::block_on(async {
        let mut dos = session().await;
        dos.cpu().cpu_state().set_eflags(0x647);
        assert_eq!(
            service(&mut dos, 0x3300, 0, 0, 0xabff).registers.edx as u16,
            0xab00
        );
        assert_eq!(service(&mut dos, 0x3301, 0, 0, 1).flags.eflags(), 0x647);
        assert_eq!(service(&mut dos, 0x3302, 0, 0, 0).registers.edx as u16, 1);
        assert_eq!(service(&mut dos, 0x3300, 0, 0, 0).registers.edx as u16, 0);
        let bad = service(&mut dos, 0x33ff, 0, 0, 0);
        assert_eq!(bad.registers.eax as u16, 0x33ff);
        assert_eq!(bad.flags.eflags(), 0x647);
        service(&mut dos, 0x3301, 0, 0, 1);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(service(&mut dos, 0x3300, 0, 0, 0).registers.edx as u16, 1);
    });
}

#[test]
fn checked_console_services_use_the_default_terminating_handler() {
    pollster::block_on(async {
        for function in [
            0x0100, 0x0200, 0x0800, 0x0900, 0x0a00, 0x0b00, 0x3f00, 0x4000,
        ] {
            let mut dos = session().await;
            dos.cpu()
                .memory()
                .view()
                .write(0x10400, if function == 0x0a00 { &[8, 0] } else { b"X$" })
                .unwrap();
            start(&mut dos, function);
            if function == 0x4000 {
                dos.cpu().cpu_state().set_bx(1);
            }
            dos.stdin().write_all(&[3]).unwrap();
            assert_eq!(
                dos.cpu().run().await,
                ExecutionExit::Host(HostExit::Return),
                "AH={function:04x}"
            );
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(Ok(0))
            );
            assert_eq!(common::output(dos.stdout()), b"^C\r\n");
        }
    });
}

#[test]
fn unchecked_character_and_raw_handle_reads_return_control_c() {
    pollster::block_on(async {
        let mut dos = session().await;
        service(&mut dos, 0x3301, 0, 0, 1);
        for function in [0x0600, 0x0700] {
            dos.stdin().write_all(&[3]).unwrap();
            assert_eq!(
                service(&mut dos, function, 0, 0, 0xff).registers.eax as u8,
                3
            );
        }
        service(&mut dos, 0x3301, 0, 0, 0);
        service(&mut dos, 0x4401, 0, 0, 0xe3);
        dos.stdin().write_all(&[3]).unwrap();
        assert_eq!(common::success(&service(&mut dos, 0x3f00, 0, 1, 0x400)), 1);
        assert_eq!(dos.cpu().memory().view().read_u8(0x10400).unwrap(), 3);
        assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        assert!(common::output(dos.stdout()).is_empty());
    });
}

#[test]
fn int23_receives_user_registers_stack_and_clear_indos_for_iret_and_retf() {
    pollster::block_on(async {
        for returns in [
            &[0xcf][..],
            &[0xf8, 0xcb],
            &[0xf9, 0xcb],
            &[0xf9, 0xca, 2, 0],       // RETF 2 restores SP, so carry is ignored.
            &[0x83, 0x4e, 4, 1, 0xcf], // IRET ignores carry in its saved FLAGS.
        ] {
            let mut dos = session().await;
            let result = service(&mut dos, 0x3400, 0, 0, 0);
            let indos = FarPointer {
                segment: result.segments[Segment::Es].selector,
                offset: result.registers.ebx as u16,
            };
            let mut handler = vec![
                0xa3, 0, 3, // Original AX.
                0x89, 0x16, 2, 3, // Original DX.
                0x89, 0x26, 4, 3, // Entry SP.
                0x9c, 0x58, 0xa3, 6, 3, // Entry FLAGS.
                0x89, 0xe5, // BP = entry SP.
                0x8b, 0x46, 4, 0xa3, 0x10, 3, // Saved INT23 FLAGS.
                0x8b, 0x46, 6, 0xa3, 0x0a, 3, // Original INT21 return IP.
                0x8b, 0x46, 8, 0xa3, 0x0c, 3, // Original INT21 return CS.
                0x8b, 0x46, 10, 0xa3, 0x0e, 3, // Original INT21 return FLAGS.
                0x06, 0x53, // Save ES/BX.
                0xb8,
            ];
            handler.extend(indos.segment.to_le_bytes());
            handler.extend([0x8e, 0xc0, 0xbb]);
            handler.extend(indos.offset.to_le_bytes());
            handler.extend([
                0x26, 0x8a, 7, 0xa2, 8, 3, // Save InDOS before nested service.
                0x5b, 0x07, // Restore BX/ES.
                0xb8, 0, 0x30, 0xcd, 0x21, // Nested DOS version service is allowed.
                0xa1, 0, 3, // Restore original request AX.
            ]);
            handler.extend(returns);
            install(&mut dos, &handler);
            start(&mut dos, 0x0800);
            dos.cpu().cpu_state().set_eflags(0x647);
            dos.stdin().write_all(b"\x03K").unwrap();
            let terminate = returns == [0xf9, 0xcb];
            assert_eq!(
                common::run_until(dos.cpu(), 0x102).await,
                if terminate {
                    ExecutionExit::Host(HostExit::Return)
                } else {
                    ExecutionExit::Interrupted
                }
            );
            let memory = dos.cpu().memory();
            assert_eq!(memory.view().read_u16(0x10300).unwrap(), 0x0800);
            assert_eq!(memory.view().read_u16(0x10302).unwrap(), 0x400);
            assert_eq!(memory.view().read_u16(0x10304).unwrap(), 0xfff2);
            assert_eq!(memory.view().read_u16(0x10306).unwrap() & 0x601, 0);
            assert_eq!(memory.view().read_u16(0x10310).unwrap() & 0x601, 0);
            assert_eq!(memory.view().read_u16(0x1030a).unwrap(), 0x102);
            assert_eq!(memory.view().read_u16(0x1030c).unwrap(), 0x1000);
            assert_eq!(memory.view().read_u16(0x1030e).unwrap(), 0x647);
            assert_eq!(memory.view().read_u8(0x10308).unwrap(), 0);
            assert_eq!(memory.view().read_u8(indos.address()).unwrap(), 0);
            if !terminate {
                assert_eq!(dos.cpu().cpu_state().ax(), 0x084b);
                assert_eq!(dos.cpu().cpu_state().sp(), 0xfffe);
                assert_eq!(dos.cpu().cpu_state().eflags(), 0x647);
            }
        }
    });
}

#[test]
fn returning_break_handler_restarts_line_editing_and_repeats_the_requested_flush() {
    pollster::block_on(async {
        let mut dos = session().await;
        install(&mut dos, &[0xcf]);
        dos.cpu().memory().view().write(0x10400, &[8, 0]).unwrap();
        dos.stdin().write_all(b"ab\x03").unwrap();
        start(&mut dos, 0x0a00);
        common::assert_pending(dos.cpu().run());
        assert!(dos.cpu().cpu_state().interrupts_enabled());
        dos.stdin().write_all(b"\tK\r").unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        let mut line = [0; 5];
        dos.cpu().memory().view().read(0x10400, &mut line).unwrap();
        assert_eq!(line, [8, 2, b'\t', b'K', b'\r']);
        assert_eq!(common::output(dos.stdout()), b"ab^C\r\n        K\r");

        for function in [0x0c08, 0x0c0a] {
            start(&mut dos, function);
            common::assert_pending(dos.cpu().run());
            dos.stdin()
                .write_all(if function == 0x0c08 {
                    b"\x03K"
                } else {
                    b"ab\x03K"
                })
                .unwrap();
            common::assert_pending(dos.cpu().run());
            // Both reads repeat AH=0Ch's flush, discarding K after the break.
            dos.stdin()
                .write_all(if function == 0x0c08 { b"N" } else { b"N\r" })
                .unwrap();
            assert_eq!(
                common::run_until(dos.cpu(), 0x102).await,
                ExecutionExit::Interrupted
            );
            if function == 0x0c08 {
                assert_eq!(dos.cpu().cpu_state().al(), b'N');
            } else {
                let mut line = [0; 4];
                dos.cpu().memory().view().read(0x10400, &mut line).unwrap();
                assert_eq!(line, [8, 1, b'N', b'\r']);
            }
        }
    });
}

#[test]
fn break_restart_observes_callback_changes_to_redirection_and_guest_buffers() {
    pollster::block_on(async {
        for function in [0x0a00, 0x3f00, 0x4000] {
            let mut dos = session().await;
            dos.mount_file(
                "IN.BIN",
                b"XYZ\r".to_vec(),
                relocade_dos::FileAccess::ReadOnly,
            )
            .unwrap();
            dos.mount_file("OUT.BIN", vec![], relocade_dos::FileAccess::ReadWrite)
                .unwrap();
            let input = common::success(&common::path_service(&mut dos, b"IN.BIN", 0x3d00, 0));
            let output = common::success(&common::path_service(&mut dos, b"OUT.BIN", 0x3d01, 0));
            let mut interrupts = Interrupts::new(dos.cpu());
            interrupts.register_interrupt(
                0x23,
                FarPointer {
                    segment: 0x1000,
                    offset: 0x180,
                },
                async move |call| {
                    call.cpu
                        .with_saved_registers([reg::ax, reg::bx, reg::cx, reg::dx], async {
                            call.memory
                                .write(
                                    0x10400,
                                    if function == 0x0a00 {
                                        &[3, 0][..]
                                    } else {
                                        b"NEW"
                                    },
                                )
                                .unwrap();
                            call.cpu.set_ax(0x4600);
                            call.cpu
                                .set_bx(if function == 0x4000 { output } else { input });
                            call.cpu.set_cx(u16::from(function == 0x4000));
                            call.call_interrupt(0x21).await;
                            assert!(!call.cpu.carry());
                            if function == 0x0a00 {
                                call.cpu.set_ax(0x4600);
                                call.cpu.set_bx(output);
                                call.cpu.set_cx(1);
                                call.call_interrupt(0x21).await;
                                assert!(!call.cpu.carry());
                            }
                        })
                        .await;
                },
            );
            interrupts.install().await.unwrap();
            dos.cpu()
                .memory()
                .view()
                .write(
                    0x10400,
                    if function == 0x0a00 {
                        &[8, 0][..]
                    } else {
                        b"O\x1aX"
                    },
                )
                .unwrap();
            dos.stdin()
                .write_all(if function == 0x4000 {
                    &b"\x03"[..]
                } else {
                    b"ab\x03"
                })
                .unwrap();
            let result = service(&mut dos, function, u16::from(function == 0x4000), 3, 0x400);
            let mut bytes = [0; 5];
            dos.cpu().memory().view().read(0x10400, &mut bytes).unwrap();
            match function {
                0x0a00 => {
                    assert_eq!(bytes, [3, 2, b'X', b'Y', b'\r']);
                    assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"XY\x07\r");
                }
                0x3f00 => {
                    assert_eq!(common::success(&result), 3);
                    assert_eq!(&bytes[..3], b"XYZ");
                }
                0x4000 => {
                    assert_eq!(common::success(&result), 3);
                    assert_eq!(dos.read_file("OUT.BIN").unwrap(), b"NEW");
                }
                _ => unreachable!(),
            }
            assert_eq!(
                common::output(dos.stdout()),
                if function == 0x4000 {
                    &b"^C\r\n"[..]
                } else {
                    b"ab^C\r\n"
                }
            );
        }
    });
}

#[test]
fn break_on_extends_checks_to_disk_services_with_redirected_input() {
    pollster::block_on(async {
        let mut dos = session().await;
        dos.mount_file("IN.TXT", b"Z".to_vec(), relocade_dos::FileAccess::ReadOnly)
            .unwrap();
        let handle = common::success(&common::path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        service(&mut dos, 0x4600, handle, 0, 0);
        dos.stdin().write_all(&[3]).unwrap();
        assert_eq!(service(&mut dos, 0x3000, 0, 0, 0).registers.eax as u16, 5);
        // AH=33h itself neither checks Ctrl-C nor consumes it.
        service(&mut dos, 0x3301, 0, 0, 1);
        start(&mut dos, 0x3000);
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(dos.read_file("IN.TXT").unwrap(), b"Z");
    });
}

#[test]
fn checked_status_and_output_peek_redirected_input_without_skipping_ordinary_bytes() {
    pollster::block_on(async {
        let mut dos = session().await;
        dos.mount_file(
            "IN.TXT",
            b"\x03K".to_vec(),
            relocade_dos::FileAccess::ReadOnly,
        )
        .unwrap();
        let handle = common::success(&common::path_service(&mut dos, b"IN.TXT", 0x3d00, 0));
        service(&mut dos, 0x4600, handle, 0, 0);
        install(&mut dos, &[0xcf]);
        for function in [0x0b00, 0x0200, 0x0900, 0x4000] {
            service(&mut dos, 0x4200, handle, 0, 0);
            dos.cpu().memory().view().write(0x10400, b"X$").unwrap();
            let cpu = service(
                &mut dos,
                function,
                1,
                1,
                if function == 0x0200 {
                    u16::from(b'X')
                } else {
                    0x400
                },
            );
            if function == 0x0b00 {
                assert_eq!(cpu.registers.eax as u8, 0xff);
            }
            assert_eq!(
                common::output(dos.stdout()),
                if function == 0x0b00 {
                    &b"^C\r\n"[..]
                } else {
                    &b"^C\r\nX"[..]
                }
            );
            assert_eq!(common::success(&service(&mut dos, 0x4201, handle, 0, 0)), 1);
            assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
            assert_eq!(common::success(&service(&mut dos, 0x4201, handle, 0, 0)), 1);
        }
    });
}
