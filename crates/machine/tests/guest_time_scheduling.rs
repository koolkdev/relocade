mod support;

use futures_util::future::join;
use relocade_machine::{
    timer_service, Clock, Device, DispatchEntry, ExitHandle, Hook, Instant, Runner, Timestamp,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    num::NonZeroU64,
    pin::pin,
    rc::Rc,
    task::Poll,
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn configuration() -> support::Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn prepare_cpu(runner: &mut Runner, code: &[u8]) {
    let memory = runner.memory();
    memory.view().write(0x100, code).unwrap();
}

async fn unpaced_slice_overshoot() {
    for (rate, slice, elapsed) in [
        (
            3,
            Duration::from_secs(1),
            Duration::from_nanos(3_333_333_333),
        ),
        (
            3_000_000_000,
            Duration::from_nanos(1),
            Duration::from_nanos(3),
        ),
    ] {
        let setup = configuration();
        let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
        let prepared = machine.create_cpu(setup.cpu, None).unwrap();
        let worker = prepared.spawn_with_guest_time(
            move || {
                relocade_machine::GuestTimeConfig::new(NonZeroU64::new(rate).unwrap())
                    .with_slice_duration(slice)
            },
            async move |mut runner| {
                let clock = runner.clock().clone();
                let start = clock.now();
                let code = [0x40, 0xeb, 0xfd]; // INC AX; JMP 0100.
                prepare_cpu(&mut runner, &code).await;
                let cpu = runner.cpu();
                let block = Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x100, &code, 2)
                    .unwrap();
                cpu.install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        address: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
                let state = cpu.cpu_state();
                let stop = cpu.exit_handle();
                let mut samples = Vec::new();
                let observe = poll_fn(|_| {
                    let count = state.instruction_count();
                    if count != 0 && samples.last() != Some(&count) {
                        samples.push(count);
                    }
                    if samples.len() == 3 {
                        stop.request();
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                });
                let ((), exit) = join(observe, runner.run_cpu()).await;
                assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
                // Two-instruction blocks cross the targets 3, 6 and 9. Overshoot
                // must shorten the next allowance instead of shifting the timebase.
                assert_eq!(samples, [4, 6, 10]);
                assert_eq!(
                    runner.clock().duration_since(runner.clock().now(), start),
                    elapsed
                );
            },
        );
        assert_eq!(worker.join().await.unwrap(), Some(()));
        machine.shutdown().await.unwrap();
    }
}

struct Deadlines {
    clock: Clock,
    deadlines: Vec<Timestamp>,
    observed: Vec<Timestamp>,
    exit: Option<ExitHandle>,
    exit_after_waits: bool,
}

impl Deadlines {
    fn configure_io(io: &mut relocade_machine::DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let callbacks = relocade_machine::DeviceCallbacks::new(device);
        io.bind(
            0x80,
            &relocade_machine::PortRead::new(|_, _, _| u32::MAX),
            &relocade_machine::PortWrite::low_byte(callbacks.write(|_, device, _| device.signal())),
        );
    }
    fn signal(&mut self) {
        self.exit.as_ref().unwrap().request();
    }
}

impl Device for Deadlines {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        let deadlines = std::mem::take(&mut device.borrow_mut().deadlines);
        for deadline in deadlines {
            clock.sleep_until(deadline).await;
            device.borrow_mut().observed.push(clock.now());
        }
        if device.borrow().exit_after_waits {
            device.borrow().exit.as_ref().unwrap().request();
        }
    }
}

async fn deadlines_and_final_exit() {
    for final_exit in [false, true] {
        let setup = configuration();
        let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
        let prepared = machine.create_cpu(setup.cpu, None).unwrap();
        let worker = prepared.spawn_with_guest_time(
            || {
                relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                    .with_slice_duration(Duration::from_millis(10))
            },
            async move |mut runner| {
                let clock = runner.clock().clone();
                let start = clock.now();
                let device = Rc::new(RefCell::new(Deadlines {
                    clock: runner.clock().clone(),
                    deadlines: if final_exit {
                        vec![clock.add(start, Duration::from_millis(3))]
                    } else {
                        vec![
                            clock.add(start, Duration::from_micros(500)),
                            clock.add(start, Duration::from_millis(5)),
                        ]
                    },
                    observed: Vec::new(),
                    exit: None,
                    exit_after_waits: !final_exit,
                }));
                runner
                    .connect(device.clone(), |io| {
                        Deadlines::configure_io(io, &device);
                    })
                    .await;
                let code = if final_exit {
                    &[0x90, 0x90, 0xe6, 0x80][..] // Exit from OUT, crossing the deadline.
                } else {
                    &[0x40, 0xeb, 0xfd][..] // INC AX; JMP 0100.
                };
                prepare_cpu(&mut runner, code).await;
                device.borrow_mut().exit = Some(runner.cpu().exit_handle());
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                let expected = if final_exit { vec![3] } else { vec![2, 6] };
                assert_eq!(
                    device.borrow().observed,
                    expected
                        .into_iter()
                        .map(|millis| clock.add(start, Duration::from_millis(millis)))
                        .collect::<Vec<_>>()
                );
            },
        );
        assert_eq!(worker.join().await.unwrap(), Some(()));
        machine.shutdown().await.unwrap();
    }
}

struct Periodic {
    clock: Clock,
    wakes: usize,
}

impl Device for Periodic {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        loop {
            clock.sleep(Duration::from_millis(1)).await;
            device.borrow_mut().wakes += 1;
        }
    }
}

async fn retained_service() {
    let (host_time, timers) = timer_service();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_guest_time(
        || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                .with_slice_duration(Duration::from_millis(10))
        },
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Periodic {
                clock: runner.clock().clone(),
                wakes: 0,
            }));
            runner.connect(device.clone(), |_| {}).await;
            prepare_cpu(&mut runner, &[0x90]).await;
            let started = Rc::new(Cell::new(0));
            let clock = runner.clock().clone();
            let stop = runner.cpu().exit_handle();
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x100, {
                let started = started.clone();
                async move || {
                    started.set(started.get() + 1);
                    clock.sleep(Duration::from_millis(5)).await;
                    // The host timer wakes this retained service while periodic
                    // guest waits keep the execution worker continuously ready.
                    host_time.clock().sleep(Duration::from_millis(10)).await;
                    stop.request();
                }
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(started.get(), 1);
            assert!(device.borrow().wakes >= 5);
            assert!(runner.clock().now() >= runner.clock().add(start, Duration::from_millis(5)));
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
        },
    );
    let timers = timers.spawn_with_peer(worker.wake_peer());
    let result = worker.join().await.unwrap();
    timers.stop().await.unwrap();
    assert_eq!(result, Some(()));
    machine.shutdown().await.unwrap();
}

async fn unpaced_idle_service() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_guest_time(
        || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                .with_slice_duration(Duration::from_millis(1))
        },
        async |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let deadline = clock.add(start, Duration::from_millis(10));
            prepare_cpu(&mut runner, &[0x90]).await;
            let cpu = runner.cpu();
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x100, async move || {
                let mut observed = Vec::new();
                let mut sleep = pin!(clock.sleep_until(deadline));
                poll_fn(|cx| {
                    let now = clock.now();
                    if observed.last() != Some(&now) {
                        observed.push(now);
                    }
                    sleep.as_mut().poll(cx)
                })
                .await;
                // A pending service resumes at its guest deadline without
                // artificial idle-time steps at the intervening slice boundaries.
                assert_eq!(observed, [start, deadline]);
                exit.request();
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unpaced_idle_time_skips_slice_boundaries() {
    pollster::block_on(unpaced_idle_service());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unpaced_idle_time_skips_slice_boundaries() {
    unpaced_idle_service().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unpaced_slices_carry_checkpoint_overshoot_forward() {
    pollster::block_on(unpaced_slice_overshoot());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unpaced_slices_carry_checkpoint_overshoot_forward() {
    unpaced_slice_overshoot().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_deadlines_precede_slice_boundaries_and_flush_on_final_exit() {
    pollster::block_on(deadlines_and_final_exit());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_deadlines_precede_slice_boundaries_and_flush_on_final_exit() {
    deadlines_and_final_exit().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn idle_time_preserves_services_waiting_on_host_timers() {
    pollster::block_on(retained_service());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn idle_time_preserves_services_waiting_on_host_timers() {
    retained_service().await;
}

async fn immediate_exit(mut runner: Runner) {
    prepare_cpu(&mut runner, &[0x90]).await;
    let completed = Rc::new(Cell::new(false));
    let exit = runner.cpu().exit_handle();
    let cpu = runner.cpu();
    cpu.install([Hook::host(0x100, {
        let completed = completed.clone();
        async move || {
            completed.set(true);
            exit.request();
        }
    })])
    .await
    .unwrap();
    let mut run = pin!(runner.run_cpu());
    let result = poll_fn(|cx| {
        let result = run.as_mut().poll(cx);
        assert!(
            !completed.get() || result.is_ready(),
            "a terminal exit with no activity notification must return in the same poll"
        );
        result
    })
    .await;
    assert_eq!(result, ExecutionExit::Host(HostExit::Return));
}

async fn terminal_exit_without_notifications() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_guest_time(
        || relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        async |runner| immediate_exit(runner).await,
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_clock(
        || relocade_clock_test_support::ManualClock::new().clock(),
        async |runner| immediate_exit(runner).await,
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn terminal_exit_without_ready_work_adds_no_activity_turn() {
    pollster::block_on(terminal_exit_without_notifications());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn terminal_exit_without_ready_work_adds_no_activity_turn() {
    terminal_exit_without_notifications().await;
}

#[path = "guest_time_scheduling/halt.rs"]
mod halt;
#[path = "guest_time_scheduling/host.rs"]
mod host;
#[path = "guest_time_scheduling/lag.rs"]
mod lag;
#[path = "guest_time_scheduling/pacing.rs"]
mod pacing;

#[path = "guest_time_scheduling/sync.rs"]
mod sync;
