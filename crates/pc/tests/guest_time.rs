use relocade_machine::{DispatchEntry, ExecutionWorker, Hook, SessionConfig};
use relocade_pc::Pc;
use relocade_x86::{Compiler, ExecutionExit, ExecutionProfile, HostExit};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);

async fn pit_interrupts() {
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let execution = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_pc::configuration::guest_time()),
        async |runner| {
            let mut pc = Pc::new(runner).await.unwrap();
            let cpu = pc.cpu();
            let memory = cpu.memory();
            // Program Bumpy's mode-3 divisor 0951h, unmask IRQ0, then perform
            // guest work until eight interrupts have recorded its progress.
            let program = [
                0xb0, 0x36, 0xe6, 0x43, 0xb0, 0x51, 0xe6, 0x40, 0xb0, 0x09, 0xe6, 0x40, 0xb0, 0xfe,
                0xe6, 0x21, 0xfb, 0x90, 0xff, 0x06, 0x00, 0x07, // INC word [0700].
                0x83, 0x3e, 0x02, 0x07, 8, // CMP word [0702], 8.
                0x72, 0xf5, // JB to INC.
            ];
            memory.view().write(0x1000, &program).unwrap();
            memory.view().write(8 * 4, &[0, 0x20, 0, 0]).unwrap();
            // IRQ0: save AX, record main-loop progress, count the interrupt,
            // acknowledge the PIC, restore AX, and return to the busy loop.
            memory
                .view()
                .write(
                    0x2000,
                    &[
                        0x50, 0xa1, 0x00, 0x07, 0xab, 0xff, 0x06, 0x02, 0x07, 0xb0, 0x20, 0xe6,
                        0x20, 0x58, 0xcf,
                    ],
                )
                .unwrap();
            let state = cpu.cpu_state();
            state.set_eip(0x1000);
            state.set_sp(0x8000);
            state.set_di(0x600);
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x1012, &program[18..], 32)
                .unwrap();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    key: 0x1012,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
            let stop = cpu.exit_handle();
            cpu.install([Hook::host(0x101d, async move || stop.request())])
                .await
                .unwrap();
            // Start the measurement with OUT0 high and any setup-time IRQ0
            // acknowledged. The program loads the divisor for all eight samples.
            let io = cpu.io();
            io.write_port(relocade_pit::CONTROL_PORT, 1, 0x36);
            io.write_port(relocade_pic::COMMAND_PORT, 1, 0x0c); // PIC poll command.
            if io.read_port(relocade_pic::COMMAND_PORT, 1) == 0x80 {
                relocade_pic::end_of_interrupt(&io, relocade_pic::TIMER_IRQ);
            }
            let start = pc.clock().now();
            assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(memory.view().read_u16(0x702).unwrap(), 8);
            let samples = (0..8)
                .map(|index| memory.view().read_u16(0x600 + index * 2).unwrap())
                .collect::<Vec<_>>();
            // A delayed timer can coalesce edges, and an uncapped CPU can
            // wrap the 16-bit counter. Check that main code ran between IRQs.
            assert!(samples.windows(2).any(|pair| pair[0] != pair[1]));
            // Eight periodic IRQs span at least seven real PIT periods.
            let elapsed = pc.clock().duration_between(start, pc.clock().now());
            assert!(elapsed.as_micros() >= relocade_pit::time_at(7 * 0x0951).as_micros());
        },
    );
    assert_eq!(execution.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pit_deadlines_interrupt_a_busy_pc() {
    pollster::block_on(pit_interrupts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pit_deadlines_interrupt_a_busy_pc() {
    pit_interrupts().await;
}

async fn poll_vga_retrace() {
    use relocade_clock_test_support::ManualClock;

    let host = ManualClock::new();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let execution = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_pc::configuration::guest_time())
            .with_host_clock(host.clone()),
        async move |runner| {
            let mut pc = Pc::new(runner).await.unwrap();
            let clock = pc.clock().clone();
            // Mode 03h starts its roughly 64 µs retrace during the 13th millisecond.
            let elapsed = Duration::from_millis(12);
            let start = clock.deadline_after(clock.now(), elapsed);
            host.advance_by(elapsed);
            pc.wait_for(clock.sleep_until(start)).await;
            let cpu = pc.cpu();
            let state = cpu.cpu_state();
            state.set_eip(0x1000);
            state.set_dx(0x3da);
            state.set_cx(u16::MAX);
            // Poll both retrace edges. CX bounds the guest loop if either edge is missed.
            let code = [
                0xec, 0xa8, 8, 0xe1, 0xfb, // IN AL,DX; TEST AL,8; LOOPZ 1000h
                0xe3, 5, // JCXZ 100ch
                0xec, 0xa8, 8, 0xe0, 0xfb, // IN AL,DX; TEST AL,8; LOOPNZ 1007h
            ];
            cpu.memory().view().write(0x1000, &code).unwrap();
            let compiler = Compiler::new(ExecutionProfile::Real16);
            for (at, bytes) in [(0x1000, &code[..5]), (0x1007, &code[7..])] {
                let block = compiler.compile_block(at, bytes, 3).unwrap();
                cpu.install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: at,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
            }
            let stop = cpu.exit_handle();
            cpu.install([Hook::host(0x100c, async move || stop.request())])
                .await
                .unwrap();
            assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
            assert_ne!(state.cx(), 0, "timed out polling the VGA retrace edges");
            assert_eq!(host.clock().now(), host.at(elapsed));
            assert!(clock.duration_between(start, clock.now()) < Duration::from_millis(3));
        },
    );
    assert_eq!(execution.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_guest_can_poll_both_vga_retrace_edges_between_host_samples() {
    pollster::block_on(poll_vga_retrace());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_guest_can_poll_both_vga_retrace_edges_between_host_samples() {
    poll_vga_retrace().await;
}

async fn keyboard_at_guest_receipt_time() {
    use relocade_clock_test_support::ManualClock;
    use relocade_frontend::keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent};
    use relocade_machine::{worker::wake_connection, Runner};
    use std::{
        future::{poll_fn, Future},
        pin::pin,
        task::Poll,
    };

    let host = ManualClock::new();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_wake_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::with_guest_time(
        relocade_pc::configuration::guest_time(),
        host.clock(),
        prepared,
        endpoint,
    )
    .await
    .unwrap();
    let clock = runner.clock().clone();
    let (ready, input) = async_channel::bounded(1);
    let mut session = pin!(
        runner.run(std::future::pending::<()>(), async move |runner| {
            let mut pc = Pc::new(runner).await.unwrap();
            let keyboard = pc.keyboard_input();
            let cpu = pc.cpu();
            let memory = cpu.memory();
            // Bumpy's PIT divisor, IRQ0/IRQ1 enabled, then a blocking BIOS key read.
            let program = [
                0xb0, 0x36, 0xe6, 0x43, 0xb0, 0x51, 0xe6, 0x40, 0xb0, 0x09, 0xe6, 0x40, 0xb0, 0xfc,
                0xe6, 0x21, 0xfb, 0x90, 0x31, 0xc0, 0xcd, 0x16,
            ];
            memory.view().write(0x1000, &program).unwrap();
            let state = cpu.cpu_state();
            state.set_eip(0x1000);
            state.set_sp(0x8000);
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x1016, async move || exit.request())])
                .await
                .unwrap();
            ready.try_send((keyboard, state.clone())).unwrap();
            assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
            (state.ax(), pc.clock().now())
        })
    );
    let (keyboard, state) = poll_fn(|cx| {
        assert!(session.as_mut().poll(cx).is_pending());
        input.try_recv().map_or(Poll::Pending, Poll::Ready)
    })
    .await;
    // PC setup may already have requested a device deadline. Wait until the
    // guest has run and is waiting on its own input before delivering the key.
    poll_fn(|cx| {
        assert!(session.as_mut().poll(cx).is_pending());
        if let Some(at) = host.next_deadline() {
            if state.instruction_count() != 0 {
                return Poll::Ready(());
            }
            host.advance_to(at);
        }
        Poll::Pending
    })
    .await;
    let boundary = clock.now();
    host.advance_by(Duration::from_millis(7));
    keyboard
        .unbounded_send(KeyboardEvent::Key {
            physical_key: KeyCode::KeyA.into(),
            logical_key: Key::Character("a".into()),
            location: KeyLocation::Standard,
            text: Some("a".into()),
            state: ElementState::Pressed,
            repeat: false,
        })
        .unwrap();
    // Host arrival does not timestamp the key. The scheduler publishes the
    // full elapsed interval when it next polls the device.
    host.advance_by(Duration::from_millis(2));
    assert_eq!(clock.now(), boundary);
    let (key, completed) = session.await.unwrap();
    assert_eq!(key, 0x1e61);
    assert_eq!(
        completed,
        clock.deadline_after(boundary, Duration::from_millis(9))
    );
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_keyboard_input_uses_guest_receipt_time_while_the_cpu_waits() {
    pollster::block_on(keyboard_at_guest_receipt_time());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn queued_keyboard_input_uses_guest_receipt_time_while_the_cpu_waits() {
    keyboard_at_guest_receipt_time().await;
}
