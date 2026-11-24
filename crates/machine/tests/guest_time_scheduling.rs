mod support;

use futures_util::future::Either;
use futures_util::StreamExt;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{
    timer_service, Clock, Device, DispatchEntry, ExecutionWorker, ExitHandle, GuestTimeConfig,
    Hook, Runner, SessionConfig, Timestamp,
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
        Compiler::new(ExecutionProfile::Real16).config(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn reject_guest_time_without_budgets() {
    let setup = configuration();
    let config = relocade_machine::CpuConfig::pc(*setup.cpu.cpu())
        .unwrap()
        .with_instruction_budgets(false);
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let cpu = machine.create_cpu(config, None).unwrap();
    let worker = ExecutionWorker::<()>::spawn(
        cpu,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())),
        async |_| panic!("a rejected CPU configuration must not start the session"),
    );
    assert!(matches!(
        worker.join().await,
        Err(relocade_machine::Error::Configuration(
            relocade_machine::ConfigurationError::GuestTimeRequiresInstructionBudgets
        ))
    ));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_time_requires_instruction_budgets_at_startup() {
    pollster::block_on(reject_guest_time_without_budgets());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_time_requires_instruction_budgets_at_startup() {
    reject_guest_time_without_budgets().await;
}

async fn prepare_cpu(runner: &mut Runner, code: &[u8]) {
    let memory = runner.memory();
    memory.view().write(0x100, code).unwrap();
}

async fn successive_deadlines() {
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(GuestTimeConfig::new(
            NonZeroU64::new(1_000_000_000).unwrap(),
        ))
        .with_host_clock(ManualClock::new()),
        async |mut runner| {
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
                    key: 0x100,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
            let state = cpu.cpu_state();
            let observe = async {
                let mut samples = Vec::new();
                for edge in 1..=3 {
                    let deadline = clock.deadline_after(start, Duration::from_nanos(1001) * edge);
                    clock.sleep_until(deadline).await;
                    let now = clock.now();
                    assert!(
                        now >= deadline
                            && now < clock.deadline_after(deadline, Duration::from_nanos(1001))
                    );
                    samples.push(state.instruction_count());
                }
                samples
            };

            let Either::Right(samples) = runner.run_cpu_until(observe).await else {
                panic!("busy CPU exited before all three device deadlines");
            };
            // Only completed two-instruction blocks can cross the 1001-tick deadlines.
            assert_eq!(samples, [1002, 2002, 3004]);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
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
        let worker = ExecutionWorker::spawn(
            prepared,
            SessionConfig::guest(GuestTimeConfig::new(
                NonZeroU64::new(1_000_000_000).unwrap(),
            ))
            .with_host_clock(ManualClock::new()),
            async move |mut runner| {
                let clock = runner.clock().clone();
                let start = clock.now();
                let device = Rc::new(RefCell::new(Deadlines {
                    clock: runner.clock().clone(),
                    deadlines: if final_exit {
                        vec![clock.deadline_after(start, Duration::from_nanos(1))]
                    } else {
                        vec![
                            clock.deadline_after(start, Duration::from_nanos(1)),
                            clock.deadline_after(start, Duration::from_nanos(1000)),
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
                    &[0xe6, 0x80][..] // OUT requests exit as the first guest instruction retires.
                } else {
                    &[0x40, 0xeb, 0xfd][..] // INC AX; JMP 0100.
                };
                prepare_cpu(&mut runner, code).await;
                device.borrow_mut().exit = Some(runner.cpu().exit_handle());
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                let observed = &device.borrow().observed;
                if final_exit {
                    assert_eq!(runner.cpu().cpu_state().instruction_count(), 1);
                    assert!(clock.now() > start);
                    assert_eq!(observed.as_slice(), [clock.now()]);
                } else {
                    let second = clock.deadline_after(start, Duration::from_nanos(1000));
                    assert_eq!(observed.len(), 2);
                    assert!(observed[0] > start && observed[0] < second);
                    assert!(observed[1] >= second);
                }
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
    let (host, mut armed, source) = host::host();
    let host_time = host.clone();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()))
            .with_host_clock(source),
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
                    // Periodic device deadlines also advance the host timer this call awaits.
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
    while let Some(deadline) = armed.next().await {
        host.advance_to(deadline);
    }
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

async fn prompt_between_cpu_runs() {
    use futures_channel::oneshot;

    let (host, mut armed, source) = host::host();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()))
            .with_host_clock(source),
        async |mut runner| {
            let clock = runner.clock().clone();
            let state = runner.cpu().cpu_state();
            let exit = runner.cpu().exit_handle();
            prepare_cpu(&mut runner, &[0x40, 0xeb, 0]).await; // INC AX; JMP 0103.
            runner
                .cpu()
                .install([Hook::host(0x103, async move || exit.request())])
                .await
                .unwrap();
            for count in [0, 2] {
                let deadline = clock.deadline_after(clock.now(), Duration::from_millis(7));
                let (send, prompt) = oneshot::channel();
                runner
                    .start_task({
                        let clock = clock.clone();
                        async move {
                            clock.sleep_until(deadline).await;
                            send.send(clock.now()).unwrap();
                        }
                    })
                    .await;
                assert_eq!(runner.wait_for(prompt).await.unwrap(), deadline);
                assert_eq!(state.instruction_count(), count);
                state.set_eip(0x100);
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
            }
            assert_eq!(state.instruction_count(), 4);
            assert_eq!(state.ax(), 2);
        },
    );
    while let Some(deadline) = armed.next().await {
        host.advance_to(deadline);
    }
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_prompt_receives_device_wakes_before_and_between_cpu_runs() {
    pollster::block_on(prompt_between_cpu_runs());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn the_prompt_receives_device_wakes_before_and_between_cpu_runs() {
    prompt_between_cpu_runs().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_compiled_loop_reaches_successive_guest_deadlines() {
    pollster::block_on(successive_deadlines());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_compiled_loop_reaches_successive_guest_deadlines() {
    successive_deadlines().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_deadlines_wake_during_execution_and_on_final_exit() {
    pollster::block_on(deadlines_and_final_exit());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_deadlines_wake_during_execution_and_on_final_exit() {
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
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()))
            .with_host_clock(ManualClock::new()),
        async |runner| immediate_exit(runner).await,
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::host().with_host_clock(relocade_clock_test_support::ManualClock::new()),
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
