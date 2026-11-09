use super::{host::*, *};
use futures_channel::oneshot;
use futures_util::{
    future::{select, Either},
    StreamExt,
};

async fn blocked_cpu(runner: &Runner, io: IoBus) -> Cpu {
    let mut cpu = cpu(runner, &[0x90], io).await;
    cpu.install([Hook::host(0x100, async || {
        std::future::pending::<()>().await
    })])
    .await
    .unwrap();
    cpu
}

async fn late_host_wake() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1000).unwrap())
                .with_slice_duration(Duration::from_millis(5))
                .with_host_clock(Clock::new(source))
        },
        async |runner| {
            let start = runner.clock().now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: [2, 4, 6, 8, 10, 12]
                    .map(|millis| start + Duration::from_millis(millis))
                    .to_vec(),
                observed: Vec::new(),
                exit: None,
                exit_after_waits: true,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            let mut cpu = blocked_cpu(runner, io).await;
            device.borrow_mut().exit = Some(cpu.exit_handle());
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            let observed = device
                .borrow()
                .observed
                .iter()
                .map(|time| time.duration_since(start))
                .collect::<Vec<_>>();
            assert_eq!(cpu.cpu_state().instruction_count(), 0);
            observed
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(5))
    );
    host.advance_by(Duration::from_millis(11));
    // Deadlines remain anchored even though the first host wake is late.
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(10))
    );
    assert_eq!(
        worker.join().await.unwrap(),
        Some(vec![
            Duration::from_millis(2),
            Duration::from_millis(4),
            Duration::from_millis(6),
            Duration::from_millis(8),
            Duration::from_millis(10),
            Duration::from_millis(12)
        ])
    );
    assert_eq!(host.next_deadline(), None);
}

struct ReplacedDeadline {
    clock: Clock,
    replace: Option<oneshot::Receiver<()>>,
    replaced: Option<oneshot::Sender<()>>,
    observed: Vec<Duration>,
    exit: Option<ExitHandle>,
}

impl Device for ReplacedDeadline {
    async fn run(device: Rc<RefCell<Self>>) {
        let clock = device.borrow().clock.clone();
        let start = clock.now();
        let replace = device.borrow_mut().replace.take().unwrap();
        {
            let old = pin!(clock.sleep_until(start + Duration::from_millis(15)));
            match select(old, pin!(replace)).await {
                Either::Right((Ok(()), _)) => {}
                _ => panic!("the old deadline must be replaced before it fires"),
            }
        }
        device
            .borrow_mut()
            .observed
            .push(clock.now().duration_since(start));
        device
            .borrow_mut()
            .replaced
            .take()
            .unwrap()
            .send(())
            .unwrap();
        clock.sleep_until(start + Duration::from_millis(5)).await;
        device
            .borrow_mut()
            .observed
            .push(clock.now().duration_since(start));
        device.borrow().exit.as_ref().unwrap().request();
    }
}

async fn replace_idle_deadline() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let (replace, replacement) = oneshot::channel();
    let (replaced, replacement_done) = oneshot::channel();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |runner| {
            let device = Rc::new(RefCell::new(ReplacedDeadline {
                clock: runner.clock().clone(),
                replace: Some(replacement),
                replaced: Some(replaced),
                observed: Vec::new(),
                exit: None,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            let mut cpu = blocked_cpu(runner, io).await;
            device.borrow_mut().exit = Some(cpu.exit_handle());
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            let result = device.borrow().observed.clone();
            result
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(4))
    );
    host.advance_by(Duration::from_millis(2));
    replace.send(()).unwrap();
    replacement_done.await.unwrap();
    assert_eq!(
        host.next_deadline(),
        Some(host_start + Duration::from_millis(4)),
        "an activity notification must retain the existing pacing wait"
    );
    host.advance_by(Duration::from_millis(2));
    assert_eq!(
        worker.join().await.unwrap(),
        Some(vec![Duration::from_millis(4), Duration::from_millis(5)])
    );
    assert_eq!(host.next_deadline(), None);
}

async fn cancelled_run_rebinds() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let (pause, paused) = oneshot::channel();
    let (stopped, stop) = oneshot::channel();
    let (resume, resumed) = oneshot::channel();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |runner| {
            let start = runner.clock().now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: vec![start + Duration::from_millis(10)],
                observed: Vec::new(),
                exit: None,
                exit_after_waits: true,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            let mut cpu = blocked_cpu(runner, io).await;
            device.borrow_mut().exit = Some(cpu.exit_handle());
            {
                match select(pin!(runner.run_cpu(&mut cpu)), pin!(paused)).await {
                    Either::Right((Ok(()), _)) => {}
                    _ => panic!("the run must still be waiting at cancellation"),
                }
            }
            stopped.send(()).unwrap();
            resumed.await.unwrap();
            assert_eq!(runner.clock().now(), start + Duration::from_millis(4));
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(
                device.borrow().observed,
                [start + Duration::from_millis(10)]
            );
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(4))
    );
    pause.send(()).unwrap();
    stop.await.unwrap();
    assert_eq!(
        host.next_deadline(),
        None,
        "cancelling execution must cancel its host wait"
    );
    host.advance_by(Duration::from_secs(1));
    resume.send(()).unwrap();
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(1004))
    );
    host.advance_by(Duration::from_millis(10));
    assert_eq!(worker.join().await.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn late_host_wakes_service_each_guest_deadline_without_shifting_the_mapping() {
    pollster::block_on(late_host_wake());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn late_host_wakes_service_each_guest_deadline_without_shifting_the_mapping() {
    late_host_wake().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn timer_replacement_preserves_the_current_burst_and_host_wait() {
    pollster::block_on(replace_idle_deadline());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn timer_replacement_preserves_the_current_burst_and_host_wait() {
    replace_idle_deadline().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelling_a_run_releases_host_wait_and_rebinding_excludes_paused_host_time() {
    pollster::block_on(cancelled_run_rebinds());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_a_run_releases_host_wait_and_rebinding_excludes_paused_host_time() {
    cancelled_run_rebinds().await;
}

async fn busy_bursts() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let (stop, stopped) = oneshot::channel();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || {
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(750).unwrap())
                .with_host_clock(Clock::new(source))
        },
        async move |runner| {
            let code = [0x40, 0xeb, 0xfd]; // Two-instruction loop, with checkpoint overshoot.
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
            let exit = cpu.exit_handle();
            let ((), result) = join(
                async move {
                    stopped.await.unwrap();
                    exit.request();
                },
                runner.run_cpu(&mut cpu),
            )
            .await;
            assert_eq!(result, ExecutionExit::Host(HostExit::Return));
            cpu.cpu_state().instruction_count()
        },
    );
    // At 750 instructions/s, four retired instructions cross the first 4 ms
    // boundary. The host wait includes that overshoot.
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_nanos(5_333_333))
    );
    host.advance_by(Duration::from_millis(9));
    // A late wake keeps the 4 ms grid, rather than granting another 4 ms
    // relative to the preceding overshoot or current host time.
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(8))
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_nanos(13_333_333))
    );
    stop.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), Some(10));
    assert_eq!(host.next_deadline(), None);
}

async fn ready_service_during_pacing() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let (complete, completion) = oneshot::channel();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |runner| {
            let start = runner.clock().now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: vec![start + Duration::from_millis(10)],
                observed: Vec::new(),
                exit: None,
                exit_after_waits: false,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            let mut cpu = cpu(runner, &[0x90], io).await;
            let exit = cpu.exit_handle();
            let completion = Rc::new(RefCell::new(Some(completion)));
            let clock = runner.clock().clone();
            cpu.install([Hook::host(0x100, async move || {
                let completion = completion.borrow_mut().take().unwrap();
                completion.await.unwrap();
                // Guest time is ahead of the host while this pacing wait remains
                // unresolved. Synchronization must not wait for host progress.
                clock.sync().await;
                clock.sync_to(host_start).await;
                exit.request();
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.clock().now(), start + Duration::from_millis(4));
            assert_eq!(cpu.cpu_state().instruction_count(), 0);
            assert!(device.borrow().observed.is_empty());
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(4))
    );
    complete.send(()).unwrap();
    // Completion must be polled without waiting for host time to catch up.
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(host.clock().now(), host_start);
    assert_eq!(host.next_deadline(), None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn active_execution_preserves_burst_boundaries_and_checkpoint_overshoot() {
    pollster::block_on(busy_bursts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn active_execution_preserves_burst_boundaries_and_checkpoint_overshoot() {
    busy_bursts().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_ready_host_service_can_exit_during_a_pacing_wait() {
    pollster::block_on(ready_service_during_pacing());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_ready_host_service_can_exit_during_a_pacing_wait() {
    ready_service_during_pacing().await;
}
