mod support;

use futures_util::future::join;
use relocade_machine::{
    timer_service, Clock, Device, DispatchEntry, ExitHandle, Hook, Runner, Timestamp,
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

async fn unpaced_deadline_overshoot() {
    for (rate, slice, elapsed, instruction_budgets, expected) in [
        (
            3,
            Duration::from_secs(1),
            Duration::from_nanos(3_333_333_333),
            true,
            [4, 6, 10],
        ),
        (
            3_000_000_000,
            Duration::from_nanos(17),
            Duration::from_nanos(51),
            false,
            [52, 102, 154],
        ),
    ] {
        let setup = configuration();
        let compiler =
            Compiler::new(ExecutionProfile::Real16).with_instruction_budgets(instruction_budgets);
        let config = relocade_machine::CpuConfig::new(compiler.target(), *setup.cpu.cpu()).unwrap();
        let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
        let prepared = machine.create_cpu(config, None).unwrap();
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
                let block = compiler.compile_block(0x100, &code, 2).unwrap();
                cpu.install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
                let state = cpu.cpu_state();
                let stop = cpu.exit_handle();
                let observe = async {
                    let mut samples = Vec::new();
                    for edge in 1..=3 {
                        clock
                            .sleep_until(clock.deadline_after(start, slice * edge))
                            .await;
                        samples.push(state.instruction_count());
                    }
                    stop.request();
                    samples
                };
                let (samples, exit) = join(observe, runner.run_cpu()).await;
                assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
                // Two-instruction blocks cross odd deadlines by one instruction.
                // Without budget checks the same distant deadline must survive
                // repeated block-end interrupt acknowledgements.
                assert_eq!(samples, expected);
                assert_eq!(
                    runner.clock().duration_between(start, runner.clock().now()),
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
                        vec![clock.deadline_after(start, Duration::from_millis(3))]
                    } else {
                        vec![
                            clock.deadline_after(start, Duration::from_micros(500)),
                            clock.deadline_after(start, Duration::from_millis(5)),
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
                        .map(|millis| clock.deadline_after(start, Duration::from_millis(millis)))
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
            runner.connect(device.clone(), |_| {}).await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(started.get(), 1);
            assert!(device.borrow().wakes >= 5);
            assert!(
                runner.clock().now()
                    >= runner
                        .clock()
                        .deadline_after(start, Duration::from_millis(5))
            );
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
            let deadline = clock.deadline_after(start, Duration::from_millis(10));
            prepare_cpu(&mut runner, &[0x90]).await;
            let cpu = runner.cpu();
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x100, async move || {
                #[cfg(target_arch = "wasm32")]
                let browser_turn = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(
                    &wasm_bindgen::JsValue::UNDEFINED,
                ));
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
                #[cfg(target_arch = "wasm32")]
                assert!(
                    futures_util::FutureExt::now_or_never(browser_turn).is_none(),
                    "an unpaced guest deadline can complete in the same browser task"
                );
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

async fn parked_session() {
    use futures_channel::oneshot;
    use futures_lite::future::poll_once;
    use futures_util::future::{select, Either};
    use relocade_machine::{worker::wake_connection, GuestTimeConfig};

    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::with_guest_time(
        GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        prepared,
        endpoint,
    )
    .await
    .unwrap();
    runner.memory().view().write(0x100, &[0xf4]).unwrap(); // HLT.
    let (ready, running) = oneshot::channel();
    let mut session = Box::pin(runner.run(
        std::future::pending::<()>(),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let deadline = clock.deadline_after(clock.now(), Duration::from_millis(7));
            clock.sleep_until(deadline).await;
            assert_eq!(clock.now(), deadline);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
            assert_eq!(runner.cpu().cpu_state().eip(), 0x100);
            assert!(ready.send(runner).is_ok());
            std::future::pending::<()>().await;
        },
    ));

    let mut runner = match select(running, session.as_mut()).await {
        Either::Left((Ok(runner), _)) => runner,
        _ => panic!("the session must survive its first guest deadline"),
    };
    {
        let mut foreground = pin!(runner.run_cpu());
        assert!(poll_once(foreground.as_mut()).await.is_none());
        assert!(poll_once(session.as_mut()).await.is_none());
        drop(session);
        assert_eq!(
            poll_once(foreground).await,
            Some(ExecutionExit::Interrupted)
        );
    }
    assert_eq!(runner.cpu().cpu_state().instruction_count(), 1);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parked_session_drives_guest_time_and_wakes_its_cpu_waiter_at_shutdown() {
    pollster::block_on(parked_session());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn parked_session_drives_guest_time_and_wakes_its_cpu_waiter_at_shutdown() {
    parked_session().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unpaced_deadlines_preserve_their_grid_across_checkpoint_overshoot() {
    pollster::block_on(unpaced_deadline_overshoot());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unpaced_deadlines_preserve_their_grid_across_checkpoint_overshoot() {
    unpaced_deadline_overshoot().await;
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
#[path = "guest_time_scheduling/ordering.rs"]
mod ordering;
#[path = "guest_time_scheduling/output.rs"]
mod output;
#[path = "guest_time_scheduling/pacing.rs"]
mod pacing;
