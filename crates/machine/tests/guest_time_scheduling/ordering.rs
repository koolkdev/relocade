//! Ordering of device wakes, guest execution and idle deadline advancement.

use super::*;
use futures_util::StreamExt;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{GuestTimeConfig, PortRead, PortWrite};
use std::task::{Context, Waker};

async fn polling_a_live_clock_register() {
    struct TimerPort;
    impl Device for TimerPort {}

    use std::sync::atomic::Ordering;

    let (host, _armed, source) = host::host();
    let host_reads = source.reads.clone();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_machine::GuestTimeConfig::new(
            NonZeroU64::new(1_000_000_000).unwrap(),
        ))
        .with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let state = runner.cpu().cpu_state();
            let observed = Rc::new(RefCell::new(Vec::new()));
            let samples = observed.clone();
            runner.io().connect(Rc::new(RefCell::new(TimerPort)), |io| {
                io.bind(
                    0x80,
                    &PortRead::new(move |_, _, _| {
                        let before = host_reads.load(Ordering::Relaxed);
                        let time = clock.now();
                        assert_eq!(host_reads.load(Ordering::Relaxed), before);
                        samples.borrow_mut().push((state.instruction_count(), time));
                        0
                    }),
                    &PortWrite::new(|_, _, _, _| {}),
                );
            });
            // Both reads are in the same compiled block with the host clock frozen.
            let code = [0xe4, 0x80, 0x90, 0x90, 0xe4, 0x80];
            prepare_cpu(&mut runner, &code).await;
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 4)
                .unwrap();
            let cpu = runner.cpu();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    key: 0x100,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x106, async move || exit.request())])
                .await
                .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            let samples = observed.borrow();
            assert_eq!(samples.len(), 2);
            assert_eq!([samples[0].0, samples[1].0], [0, 3]);
            assert!(samples[1].1 > samples[0].1);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 4);
            assert_eq!(host.clock().now(), host.at(Duration::ZERO));
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn port_reads_in_one_compiled_block_observe_retirement_without_reading_host_time() {
    pollster::block_on(polling_a_live_clock_register());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn port_reads_in_one_compiled_block_observe_retirement_without_reading_host_time() {
    polling_a_live_clock_register().await;
}

async fn pending_deadline_across_calibration() {
    struct TimerPort;
    impl Device for TimerPort {}

    let (host, _armed, source) = host::host();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_machine::GuestTimeConfig::new(
            NonZeroU64::new(1_000_000).unwrap(),
        ))
        .with_host_clock(source),
        async move |mut runner| {
            let state = runner.cpu().cpu_state();
            let progress = state.clone();
            runner.io().connect(Rc::new(RefCell::new(TimerPort)), |io| {
                io.bind(
                    0x80,
                    &PortRead::new(move |_, _, _| {
                        if progress.instruction_count() == 500 {
                            let sample = host.at(Duration::from_millis(4));
                            assert_eq!(host.next_deadline(), Some(sample));
                            host.advance_to(sample);
                        }
                        0
                    }),
                    &PortWrite::new(|_, _, _, _| {}),
                );
            });
            state.set_cx(1_000);
            let code = [0xe4, 0x80, 0xe2, 0xfc]; // IN AL,80h; LOOP 0100h.
            prepare_cpu(&mut runner, &code).await;
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 2)
                .unwrap();
            runner
                .cpu()
                .install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
            let cpu = runner.cpu();
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x104, async move || exit.request())])
                .await
                .unwrap();
            let clock = runner.clock().clone();
            let deadline = clock.deadline_after(clock.now(), Duration::from_millis(3));
            assert!(matches!(
                runner.run_cpu_until(clock.sleep_until(deadline)).await,
                Either::Right(())
            ));
            // The wake occurs after the sample, within the newly projected window.
            // Snapping to wall time would fire at 500; the old target is 150,000.
            assert!((600..800).contains(&state.instruction_count()));
            assert!(clock.now() >= deadline);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_pending_device_sleep_uses_the_new_cpu_rate_after_a_sample() {
    pollster::block_on(pending_deadline_across_calibration());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_pending_device_sleep_uses_the_new_cpu_rate_after_a_sample() {
    pending_deadline_across_calibration().await;
}

async fn due_device_before_next_block() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        cpu,
        SessionConfig::guest(GuestTimeConfig::new(
            NonZeroU64::new(1_000_000_000).unwrap(),
        ))
        .with_host_clock(ManualClock::new()),
        async |mut runner| {
            // OUT is one block. The subsequent JMP supplies a second, observable turn.
            let code = [0xe6, 0x80, 0xeb, 0xfe];
            prepare_cpu(&mut runner, &code).await;
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 1)
                .unwrap();
            runner
                .cpu()
                .install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
            let clock = runner.clock().clone();
            let due = clock.deadline_after(clock.now(), Duration::from_nanos(1));
            let timer = Rc::new(RefCell::new(Deadlines {
                clock: clock.clone(),
                deadlines: vec![due],
                observed: Vec::new(),
                exit: Some(runner.cpu().exit_handle()),
                exit_after_waits: true,
            }));
            runner
                .connect(timer.clone(), |io| {
                    io.bind(
                        0x80,
                        &PortRead::new(|_, _, _| 0),
                        &PortWrite::new(move |_, _, _, _| {
                            // Changing a guest wait wakes the machine before OUT
                            // publishes retirement and wakes the one-tick timer.
                            let mut wait = pin!(clock.sleep(Duration::from_secs(1)));
                            assert!(wait
                                .as_mut()
                                .poll(&mut Context::from_waker(Waker::noop()))
                                .is_pending());
                        }),
                    );
                })
                .await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            // The activity requests exit as soon as it observes the deadline.
            let timer = timer.borrow();
            assert_eq!(timer.observed.len(), 1);
            assert!(timer.observed[0] >= due);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 1);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_device_due_during_io_runs_before_the_next_guest_block() {
    pollster::block_on(due_device_before_next_block());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_device_due_during_io_runs_before_the_next_guest_block() {
    due_device_before_next_block().await;
}

async fn retained_hook_wakes_timer_owner() {
    use futures_channel::oneshot;
    use futures_util::future::{select, Either};

    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        cpu,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()))
            .with_host_clock(ManualClock::new()),
        async |mut runner| {
            let (send, signal) = oneshot::channel();
            let send = RefCell::new(Some(send));
            runner
                .cpu()
                .install([Hook::host(0x100, async move || {
                    relocade_machine::yield_now().await;
                    send.take().unwrap().send(()).unwrap();
                    std::future::pending::<()>().await;
                })])
                .await
                .unwrap();
            let exit = runner.cpu().exit_handle();
            let clock = runner.clock().clone();
            let due = clock.deadline_after(clock.now(), Duration::from_secs(20));
            runner
                .start_task(async move {
                    let timer = pin!(clock.sleep_until(due));
                    assert!(matches!(select(signal, timer).await, Either::Left(_)));
                    assert!(clock.now() < due);
                    exit.request();
                })
                .await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_device_woken_by_a_retained_hook_can_cancel_its_timer() {
    pollster::block_on(retained_hook_wakes_timer_owner());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_device_woken_by_a_retained_hook_can_cancel_its_timer() {
    retained_hook_wakes_timer_owner().await;
}

async fn new_activity_registers_earlier_deadline() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(setup.cpu, None).unwrap();
    let (host, mut armed, source) = host::host();
    let (register, registration) = futures_channel::oneshot::channel();
    let worker = ExecutionWorker::spawn(
        cpu,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()))
            .with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let later = clock.deadline_after(start, Duration::from_secs(20));
            runner.start_task(clock.sleep_until(later)).await;
            runner.wait_for(registration).await.unwrap();
            let earlier = clock.deadline_after(start, Duration::from_secs(1));
            let (send, observed) = futures_channel::oneshot::channel();
            runner
                .start_task(async move {
                    clock.sleep_until(earlier).await;
                    send.send(clock.now()).unwrap();
                })
                .await;
            assert_eq!(runner.wait_for(observed).await.unwrap(), earlier);
        },
    );
    assert_eq!(armed.next().await, Some(host.at(Duration::from_secs(20))));
    register.send(()).unwrap();
    let earlier = host.at(Duration::from_secs(1));
    assert_eq!(armed.next().await, Some(earlier));
    host.advance_to(earlier);
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_new_activity_can_register_an_earlier_idle_deadline() {
    pollster::block_on(new_activity_registers_earlier_deadline());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_new_activity_can_register_an_earlier_idle_deadline() {
    new_activity_registers_earlier_deadline().await;
}
