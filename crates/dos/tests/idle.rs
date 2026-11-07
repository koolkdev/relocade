mod common;

use common::{service, SERVICE_PROGRAM};
use relocade_dos::{Dos, ProcessStatus};
use relocade_machine::{real_mode_interrupt::Interrupts, registers as reg, CpuContext, FarPointer};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{cell::Cell, io::Write, rc::Rc};

const CALLBACK: FarPointer = FarPointer {
    segment: 0x1000,
    offset: 0x180,
};

async fn session() -> (Dos, FarPointer) {
    let mut dos = common::dos().await;
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    let state = service(&mut dos, 0x3400, 0, 0, 0);
    let indos = FarPointer {
        segment: state.segments[Segment::Es].selector,
        offset: state.registers.ebx as u16,
    };
    (dos, indos)
}

async fn install<F>(dos: &mut Dos, callback: F)
where
    F: AsyncFn(&mut CpuContext<'_>) + 'static,
{
    let mut interrupts = Interrupts::new(dos.cpu());
    interrupts.register_interrupt(0x28, CALLBACK, callback);
    interrupts.install().await.unwrap();
}

async fn queue_key(call: &mut CpuContext<'_>, byte: u8) {
    call.cpu.set_ax(0x0500);
    call.cpu.set_cx(u16::from(byte));
    call.call_interrupt(0x16).await;
    assert_eq!(call.cpu.al(), 0);
}

fn start(dos: &mut Dos, function: u16) {
    let state = dos.cpu().cpu_state();
    state.set_eip(0x100);
    state.set_ax(function);
    state.set_bx(0);
    state.set_cx(2);
    state.set_dx(0x400);
}

#[test]
fn character_waits_repeat_idle_and_retain_partial_lines_across_nested_dos_calls() {
    pollster::block_on(async {
        for function in [0x0100, 0x0700, 0x0800, 0x0a00, 0x0c08] {
            let (mut dos, indos) = session().await;
            let calls = Rc::new(Cell::new(0));
            install(&mut dos, {
                let calls = calls.clone();
                async move |call| {
                    call.cpu
                        .with_saved_registers([reg::ax, reg::bx, reg::cx], async {
                            assert_eq!(call.memory.read_u8(indos.address()).unwrap(), 1);
                            let flags = call
                                .memory
                                .read_u16(
                                    call.cpu.segment(Segment::Ss).base
                                        + u32::from(call.cpu.sp())
                                        + 4,
                                )
                                .unwrap();
                            assert_eq!(flags & 0x600, 0x200, "INT28 saves IF set and DF clear");
                            calls.set(calls.get() + 1);
                            assert!(calls.get() <= 2);
                            call.cpu.set_ax(0x3000);
                            call.call_interrupt(0x21).await;
                            assert_eq!(call.cpu.ax(), 5);
                            assert_eq!(call.memory.read_u8(indos.address()).unwrap(), 1);
                            if calls.get() == 2 {
                                queue_key(call, b'K').await;
                                if function == 0x0a00 {
                                    queue_key(call, b'\r').await;
                                }
                            }
                        })
                        .await;
                }
            })
            .await;
            dos.cpu().memory().view().write(0x10400, &[8, 0]).unwrap();
            match function {
                0x0a00 => dos.stdin().write_all(b"A").unwrap(),
                0x0c08 => dos.stdin().write_all(b"old").unwrap(),
                _ => {}
            }
            dos.cpu().cpu_state().set_eflags(0x647);
            let result = service(&mut dos, function, 0, 0, 0x400);
            assert_eq!(calls.get(), 2);
            assert_eq!(result.registers.esp as u16, 0xfffe);
            assert_eq!(result.flags.eflags(), 0x647);
            assert_eq!(
                dos.cpu().memory().view().read_u8(indos.address()).unwrap(),
                0
            );
            if function == 0x0a00 {
                let mut buffer = [0; 5];
                dos.cpu()
                    .memory()
                    .view()
                    .read(0x10400, &mut buffer)
                    .unwrap();
                assert_eq!(buffer, [8, 2, b'A', b'K', b'\r']);
            } else {
                assert_eq!(result.registers.eax as u8, b'K');
            }
        }
    });
}

#[test]
fn ready_input_and_cooked_handle_waits_do_not_invoke_idle() {
    pollster::block_on(async {
        let (mut dos, _) = session().await;
        let calls = Rc::new(Cell::new(0));
        install(&mut dos, {
            let calls = calls.clone();
            async move |_| {
                calls.set(calls.get() + 1);
            }
        })
        .await;
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(service(&mut dos, 0x0800, 0, 0, 0).registers.eax as u8, b'K');
        start(&mut dos, 0x3f00);
        common::assert_pending(dos.cpu().run());
        assert_eq!(calls.get(), 0);
        dos.stdin().write_all(b"A\r").unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(calls.get(), 0);
        let mut bytes = [0; 2];
        dos.cpu().memory().view().read(0x10400, &mut bytes).unwrap();
        assert_eq!(bytes, *b"A\r");
    });
}

#[test]
fn empty_polls_notify_once_and_report_the_status_before_the_callback() {
    pollster::block_on(async {
        for function in [0x0600, 0x0b00] {
            let (mut dos, _) = session().await;
            let calls = Rc::new(Cell::new(0));
            install(&mut dos, {
                let calls = calls.clone();
                async move |call| {
                    calls.set(calls.get() + 1);
                    call.cpu
                        .with_saved_registers([reg::ax, reg::cx], queue_key(call, b'K'))
                        .await;
                }
            })
            .await;
            dos.cpu().cpu_state().set_eflags(0x603);
            let empty = service(&mut dos, function, 0, 0, 0xff);
            assert_eq!(empty.registers.eax as u8, 0);
            assert_eq!(
                empty.flags.eflags(),
                if function == 0x0600 { 0x643 } else { 0x603 }
            );
            assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
            assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'K');
            assert_eq!(calls.get(), 1);
        }
    });
}

#[test]
fn default_idle_sleeps_and_a_replaced_vector_is_seen_by_the_waiting_read() {
    pollster::block_on(async {
        let (mut dos, indos) = session().await;
        let memory = dos.cpu().memory();
        let default = memory.view().read_u32(0x28 * 4).unwrap();
        let calls = Rc::new(Cell::new(0));
        install(&mut dos, {
            let calls = calls.clone();
            async move |call| {
                calls.set(calls.get() + 1);
                // Restoring the original vector must stop repeated callbacks.
                call.memory.write_u32(0x28 * 4, default).unwrap();
            }
        })
        .await;
        memory.view().write_u32(0x28 * 4, default).unwrap();
        start(&mut dos, 0x0700);
        common::assert_pending(dos.cpu().run());
        let instructions = dos.cpu().cpu_state().instruction_count();
        common::assert_pending(dos.cpu().run());
        assert_eq!(dos.cpu().cpu_state().instruction_count(), instructions);
        assert_eq!(calls.get(), 0);
        assert_eq!(memory.view().read_u8(indos.address()).unwrap(), 1);
        memory
            .view()
            .write_u32(0x28 * 4, CALLBACK.packed())
            .unwrap();
        common::assert_pending(dos.cpu().run());
        assert_eq!(calls.get(), 1);
        common::assert_pending(dos.cpu().run());
        assert_eq!(calls.get(), 1);
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(dos.cpu().cpu_state().al(), b'K');
        assert_eq!(memory.view().read_u8(indos.address()).unwrap(), 0);
    });
}

#[test]
fn checked_output_notifies_on_empty_input_but_handle_output_does_not() {
    pollster::block_on(async {
        let (mut dos, _) = session().await;
        let calls = Rc::new(Cell::new(0));
        install(&mut dos, {
            let calls = calls.clone();
            async move |_| {
                calls.set(calls.get() + 1);
            }
        })
        .await;
        service(&mut dos, 0x0200, 0, 0, u16::from(b'X'));
        assert_eq!(calls.get(), 1);
        dos.cpu().memory().view().write(0x10400, b"Y$").unwrap();
        service(&mut dos, 0x0900, 0, 0, 0x400);
        assert_eq!(calls.get(), 2);
        service(&mut dos, 0x4000, 1, 1, 0x400);
        assert_eq!(calls.get(), 2);
        assert_eq!(common::output(dos.stdout()), b"XYY");
    });
}

#[test]
fn critical_error_mode_suppresses_idle_and_callback_exit_releases_indos() {
    pollster::block_on(async {
        let (mut dos, indos) = session().await;
        let calls = Rc::new(Cell::new(0));
        install(&mut dos, {
            let calls = calls.clone();
            async move |call| {
                calls.set(calls.get() + 1);
                call.cpu.set_ax(0x4c2a);
                call.call_interrupt(0x21).await;
                panic!("termination must abandon the suspended idle callback");
            }
        })
        .await;
        let memory = dos.cpu().memory();
        memory.view().write_u8(indos.address() - 1, 1).unwrap();
        start(&mut dos, 0x0800);
        common::assert_pending(dos.cpu().run());
        assert_eq!(calls.get(), 0);
        memory.view().write_u8(indos.address() - 1, 0).unwrap();
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(calls.get(), 1);
        assert_eq!(memory.view().read_u8(indos.address()).unwrap(), 0);
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(42))
        );
    });
}

#[test]
fn idle_callback_can_extend_redirected_input_through_a_separate_dos_handle() {
    pollster::block_on(async {
        let (mut dos, _) = session().await;
        dos.mount_file("INPUT.BIN", vec![], relocade_dos::FileAccess::ReadWrite)
            .unwrap();
        let input = common::success(&common::path_service(&mut dos, b"INPUT.BIN", 0x3d40, 0));
        let output = common::success(&common::path_service(&mut dos, b"INPUT.BIN", 0x3d41, 0));
        common::success(&service(&mut dos, 0x4600, input, 0, 0));
        dos.cpu().memory().view().write(0x10400, b"K").unwrap();
        let calls = Rc::new(Cell::new(0));
        install(&mut dos, {
            let calls = calls.clone();
            async move |call| {
                assert_eq!(
                    calls.replace(1),
                    0,
                    "the retained read must notice the append"
                );
                call.cpu
                    .with_saved_registers([reg::ax, reg::bx, reg::cx, reg::dx], async {
                        call.cpu.set_ax(0x4000);
                        call.cpu.set_bx(output);
                        call.cpu.set_cx(1);
                        call.cpu.set_dx(0x400);
                        call.call_interrupt(0x21).await;
                        assert_eq!(call.cpu.ax(), 1);
                    })
                    .await;
            }
        })
        .await;
        assert_eq!(service(&mut dos, 0x0800, 0, 0, 0).registers.eax as u8, b'K');
        assert_eq!(calls.get(), 1);
        assert_eq!(common::success(&service(&mut dos, 0x4201, input, 0, 0)), 1);
    });
}
