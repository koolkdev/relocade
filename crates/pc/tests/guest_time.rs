#[path = "guest_time/halt_input.rs"]
mod halt_input;

use relocade_machine::{DispatchEntry, Hook};
use relocade_pc::Pc;
use relocade_x86::{Compiler, ExecutionExit, ExecutionProfile, HostExit};
use std::{num::NonZeroU64, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn pit_interrupts() {
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let worker = prepared.spawn_with_guest_time(
        || relocade_machine::GuestTimeConfig::new(NonZeroU64::new(2_000_000).unwrap()),
        async |runner| {
            let before_setup = runner.clock().now();
            let mut pc = Pc::new(runner).await.unwrap();
            assert_eq!(pc.clock().now(), before_setup);
            assert_eq!(pc.clock().tick_rate().ticks_per_second().get(), 2_000_000);
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
            let start = pc.clock().now();
            assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(memory.view().read_u16(0x702).unwrap(), 8);
            let samples = (0..8)
                .map(|index| memory.view().read_u16(0x600 + index * 2).unwrap())
                .collect::<Vec<_>>();
            assert!(samples[0] > 0);
            assert!(samples.windows(2).all(|pair| pair[0] < pair[1]));
            // PIT deadlines must drive eight approximately 2 ms periods
            // without an execution slice duration.
            assert!(
                pc.clock().now() >= pc.clock().deadline_after(start, Duration::from_millis(14))
            );
            assert!(pc.clock().now() < pc.clock().deadline_after(start, Duration::from_millis(20)));
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pit_deadlines_interrupt_a_busy_pc_without_execution_slices() {
    pollster::block_on(pit_interrupts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pit_deadlines_interrupt_a_busy_pc_without_execution_slices() {
    pit_interrupts().await;
}

async fn keyboard_at_guest_receipt_time() {
    use relocade_clock_test_support::ManualClock;
    use relocade_frontend::keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent};
    use relocade_machine::{worker::wake_connection, GuestTimeConfig, Runner};
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
        GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()).with_host_clock(host.clock()),
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
            ready.try_send(pc.keyboard_input()).unwrap();
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
            assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
            (state.ax(), pc.clock().now())
        })
    );
    // Poll the real session until its first idle host wait is registered.
    poll_fn(|cx| {
        assert!(session.as_mut().poll(cx).is_pending());
        if host.next_deadline().is_some() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    host.advance_by(Duration::from_millis(7));
    let keyboard = input.try_recv().unwrap();
    let boundary = clock.now();
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
    // Input applies at the stopped guest boundary even when host time is ahead.
    host.advance_by(Duration::from_millis(2));
    assert_eq!(clock.now(), boundary);
    let (key, completed) = session.await.unwrap();
    assert_eq!(key, 0x1e61);
    assert!(
        completed >= boundary
            && completed < clock.deadline_after(boundary, Duration::from_millis(1))
    );
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_keyboard_input_uses_guest_receipt_time_during_pacing() {
    pollster::block_on(keyboard_at_guest_receipt_time());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn queued_keyboard_input_uses_guest_receipt_time_during_pacing() {
    keyboard_at_guest_receipt_time().await;
}

async fn advancing_host_clock_without_a_machine_turn() {
    use relocade_clock_test_support::ManualClock;
    use relocade_machine::{worker::wake_connection, Clock, Runner};

    let host = ManualClock::new();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let cpu = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_wake, endpoint) = wake_connection(cpu.interrupt_handle());
    let runner = Runner::new(Clock::new(host.clone()), cpu, endpoint)
        .await
        .unwrap();
    runner
        .run(std::future::pending::<()>(), async |runner| {
            let pc = Pc::new(runner).await.unwrap();
            let io = pc.runner().io();
            let pit_count = || {
                io.write_port(0x43, 1, 0); // Latch channel 0, then read its low and high bytes.
                io.read_port(0x40, 1) | (io.read_port(0x40, 1) << 8)
            };
            let start = pc.clock().now();
            let count = pit_count();
            host.advance_by(Duration::from_millis(250));
            assert_eq!(pc.clock().now(), start);
            assert_eq!(pit_count(), count);
            assert_eq!(
                pc.clock().tick_rate().ticks_per_second(),
                relocade_pc::configuration::DEFAULT_INSTRUCTIONS_PER_SECOND
            );
        })
        .await
        .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_clock_changes_reach_the_pit_only_through_a_machine_turn() {
    pollster::block_on(advancing_host_clock_without_a_machine_turn());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn host_clock_changes_reach_the_pit_only_through_a_machine_turn() {
    advancing_host_clock_without_a_machine_turn().await;
}
