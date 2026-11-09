mod support;

use futures_util::future::join;
use relocade_machine::{
    timer_service, Clock, Cpu, Device, DispatchEntry, ExecutionWorker, ExitHandle, Hook, Instant,
    IoBus, Memory, Runner,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
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

async fn cpu(runner: &Runner, code: &[u8], io: IoBus) -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, code).unwrap();
    runner.create_cpu(memory, config.cpu, io).await.unwrap()
}

async fn yield_deadline_overshoot() {
    let worker = ExecutionWorker::spawn_with_guest_time(
        || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(3).unwrap())
                .with_yield_interval(Duration::from_secs(1))
        },
        async |runner| {
            let start = runner.clock().now();
            let code = [0x40, 0xeb, 0xfd]; // INC AX; JMP 0100.
            let mut cpu = cpu(runner, &code, IoBus::default()).await;
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
            let ((), exit) = join(observe, runner.run_cpu(&mut cpu)).await;
            assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
            // Two-instruction blocks cross the targets 3, 6 and 9. Overshoot
            // must shorten the next allowance instead of shifting the timebase.
            assert_eq!(samples, [4, 6, 10]);
            assert_eq!(
                runner.clock().now(),
                start + Duration::from_nanos(3_333_333_333)
            );
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
}

struct Deadlines {
    clock: Clock,
    deadlines: Vec<Instant>,
    observed: Vec<Instant>,
    exit: Option<ExitHandle>,
    exit_after_waits: bool,
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

    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.exit.as_ref().unwrap().request();
    }
}

async fn deadlines_and_final_exit() {
    for final_exit in [false, true] {
        let worker = ExecutionWorker::spawn_with_guest_time(
            || {
                relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                    .with_yield_interval(Duration::from_millis(10))
            },
            async move |runner| {
                let start = runner.clock().now();
                let device = Rc::new(RefCell::new(Deadlines {
                    clock: runner.clock().clone(),
                    deadlines: if final_exit {
                        vec![start + Duration::from_millis(3)]
                    } else {
                        vec![
                            start + Duration::from_micros(500),
                            start + Duration::from_millis(5),
                        ]
                    },
                    observed: Vec::new(),
                    exit: None,
                    exit_after_waits: !final_exit,
                }));
                let connection = runner
                    .connect(device.clone(), [], std::iter::once(0x80..=0x80))
                    .await;
                let code = if final_exit {
                    &[0x90, 0x90, 0xe6, 0x80][..] // Exit from OUT, crossing the deadline.
                } else {
                    &[0x40, 0xeb, 0xfd][..] // INC AX; JMP 0100.
                };
                let mut cpu = cpu(runner, code, IoBus::new([connection])).await;
                device.borrow_mut().exit = Some(cpu.exit_handle());
                assert_eq!(
                    runner.run_cpu(&mut cpu).await,
                    ExecutionExit::Host(HostExit::Return)
                );
                let expected = if final_exit { vec![3] } else { vec![2, 6] };
                assert_eq!(
                    device.borrow().observed,
                    expected
                        .into_iter()
                        .map(|millis| start + Duration::from_millis(millis))
                        .collect::<Vec<_>>()
                );
            },
        );
        assert_eq!(worker.join().await.unwrap(), Some(()));
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
    let worker = ExecutionWorker::spawn_with_guest_time(
        || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                .with_yield_interval(Duration::from_millis(10))
        },
        async move |runner| {
            let start = runner.clock().now();
            let device = Rc::new(RefCell::new(Periodic {
                clock: runner.clock().clone(),
                wakes: 0,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            let mut cpu = cpu(runner, &[0x90], io).await;
            let started = Rc::new(Cell::new(0));
            let clock = runner.clock().clone();
            let stop = cpu.exit_handle();
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
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(started.get(), 1);
            assert!(device.borrow().wakes >= 5);
            assert!(runner.clock().now() >= start + Duration::from_millis(5));
            assert_eq!(cpu.cpu_state().instruction_count(), 0);
        },
    );
    let timers = timers.spawn_with_peer(worker.wake_peer());
    let result = worker.join().await.unwrap();
    timers.stop().await.unwrap();
    assert_eq!(result, Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn yield_deadlines_carry_checkpoint_overshoot_forward() {
    pollster::block_on(yield_deadline_overshoot());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn yield_deadlines_carry_checkpoint_overshoot_forward() {
    yield_deadline_overshoot().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_deadlines_precede_yield_deadlines_and_flush_on_final_exit() {
    pollster::block_on(deadlines_and_final_exit());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_deadlines_precede_yield_deadlines_and_flush_on_final_exit() {
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

async fn immediate_exit(runner: &Runner) {
    let mut cpu = cpu(runner, &[0x90], IoBus::default()).await;
    let completed = Rc::new(Cell::new(false));
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x100, {
        let completed = completed.clone();
        async move || {
            completed.set(true);
            exit.request();
        }
    })])
    .await
    .unwrap();
    let mut run = pin!(runner.run_cpu(&mut cpu));
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
    let worker = ExecutionWorker::spawn_with_guest_time(
        || relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()),
        async |runner| immediate_exit(runner).await,
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    let worker = ExecutionWorker::spawn_with_clock(
        || relocade_clock_test_support::ManualClock::new().clock(),
        async |runner| immediate_exit(runner).await,
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
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

#[path = "guest_time_scheduling/host.rs"]
mod host;
#[path = "guest_time_scheduling/pacing.rs"]
mod pacing;
