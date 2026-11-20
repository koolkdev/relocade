use super::{host::*, *};
use futures_channel::oneshot;
use futures_util::{
    future::{select, Either},
    StreamExt,
};

async fn blocked_cpu(runner: &mut Runner) {
    prepare_cpu(runner, &[0x90]).await;
    let cpu = runner.cpu();
    cpu.install([Hook::host(0x100, async || {
        std::future::pending::<()>().await
    })])
    .await
    .unwrap();
}

async fn idle_deadlines() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_machine::GuestTimeConfig::new(
            NonZeroU64::new(1000).unwrap(),
        ))
        .with_host_clock(source),
        async |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: [2, 4, 6, 8, 10, 12]
                    .map(|millis| clock.deadline_after(start, Duration::from_millis(millis)))
                    .to_vec(),
                observed: Vec::new(),
                exit: None,
                exit_after_waits: true,
            }));
            blocked_cpu(&mut runner).await;
            device.borrow_mut().exit = Some(runner.cpu().exit_handle());
            runner.connect(device.clone(), |_| {}).await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            let observed = device
                .borrow()
                .observed
                .iter()
                .map(|time| clock.duration_between(start, *time))
                .collect::<Vec<_>>();
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
            observed
        },
    );
    // The first wake is nine milliseconds late. All elapsed deadlines share
    // the current guest reading, and the final timer keeps its original phase.
    for (scheduled, wake) in [(2, 11), (12, 12)] {
        assert_eq!(
            armed.next().await,
            Some(host_time.at(Duration::from_millis(scheduled)))
        );
        host.advance_to(host_time.at(Duration::from_millis(wake)));
    }
    assert_eq!(
        worker.join().await.unwrap(),
        Some([11, 11, 11, 11, 11, 12].map(Duration::from_millis).to_vec())
    );
    assert_eq!(host.next_deadline(), None);
    assert_eq!(armed.next().await, None);
    machine.shutdown().await.unwrap();
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
            let old =
                pin!(clock.sleep_until(clock.deadline_after(start, Duration::from_millis(15))));
            match select(old, pin!(replace)).await {
                Either::Right((Ok(()), _)) => {}
                _ => panic!("the old deadline must be replaced before it fires"),
            }
        }
        device
            .borrow_mut()
            .observed
            .push(clock.duration_between(start, clock.now()));
        device
            .borrow_mut()
            .replaced
            .take()
            .unwrap()
            .send(())
            .unwrap();
        clock
            .sleep_until(clock.deadline_after(start, Duration::from_millis(5)))
            .await;
        device
            .borrow_mut()
            .observed
            .push(clock.duration_between(start, clock.now()));
        device.borrow().exit.as_ref().unwrap().request();
    }
}

async fn replace_idle_deadline() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let (replace, replacement) = oneshot::channel();
    let (replaced, replacement_done) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(config()).with_host_clock(source),
        async move |mut runner| {
            let device = Rc::new(RefCell::new(ReplacedDeadline {
                clock: runner.clock().clone(),
                replace: Some(replacement),
                replaced: Some(replaced),
                observed: Vec::new(),
                exit: None,
            }));
            blocked_cpu(&mut runner).await;
            device.borrow_mut().exit = Some(runner.cpu().exit_handle());
            runner.connect(device.clone(), |_| {}).await;
            // Only the nested CPU waiter can replace its host sleep and observe the exit.
            let mut runs = futures_util::stream::FuturesUnordered::new();
            runs.push(runner.run_cpu());
            assert_eq!(
                runs.next().await,
                Some(ExecutionExit::Host(HostExit::Return))
            );
            let result = device.borrow().observed.clone();
            result
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(15)))
    );
    host.advance_by(Duration::from_millis(2));
    replace.send(()).unwrap();
    replacement_done.await.unwrap();
    // Replacing the sleep retains the idle origin. Only the remaining 3 ms elapse.
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(5)))
    );
    host.advance_to(host_time.at(Duration::from_millis(5)));
    assert_eq!(
        worker.join().await.unwrap(),
        Some(vec![Duration::from_millis(2), Duration::from_millis(5)])
    );
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

async fn parked_after_cancellation() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let (pause, paused) = oneshot::channel();
    let (stopped, stop) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(config()).with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: vec![clock.deadline_after(start, Duration::from_millis(10))],
                observed: Vec::new(),
                exit: None,
                exit_after_waits: true,
            }));
            blocked_cpu(&mut runner).await;
            device.borrow_mut().exit = Some(runner.cpu().exit_handle());
            runner.connect(device.clone(), |_| {}).await;
            {
                match select(pin!(runner.run_cpu()), pin!(paused)).await {
                    Either::Right((Ok(()), _)) => {}
                    _ => panic!("the run must still be waiting at cancellation"),
                }
            }
            stopped.send(()).unwrap();
            let deadline = clock.deadline_after(start, Duration::from_millis(10));
            runner.wait_for(clock.sleep_until(deadline)).await;
            assert_eq!(clock.now(), deadline);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(
                device.borrow().observed,
                [clock.deadline_after(start, Duration::from_millis(10))]
            );
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(10)))
    );
    pause.send(()).unwrap();
    stop.await.unwrap();
    assert_eq!(
        host.next_deadline(),
        Some(host_time.at(Duration::from_millis(10)))
    );
    host.advance_to(host_time.at(Duration::from_millis(10)));
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn idle_waits_target_each_guest_device_deadline() {
    pollster::block_on(idle_deadlines());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn idle_waits_target_each_guest_device_deadline() {
    idle_deadlines().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn timer_replacement_rearms_the_idle_host_wait() {
    pollster::block_on(replace_idle_deadline());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn timer_replacement_rearms_the_idle_host_wait() {
    replace_idle_deadline().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelling_cpu_execution_keeps_the_idle_device_wait() {
    pollster::block_on(parked_after_cancellation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_cpu_execution_keeps_the_idle_device_wait() {
    parked_after_cancellation().await;
}

async fn busy_bursts() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let (stop, stopped) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(
            relocade_machine::GuestTimeConfig::new(NonZeroU64::new(10_000).unwrap())
                .with_cpu_limit(NonZeroU64::new(750).unwrap()),
        )
        .with_host_clock(source),
        async move |mut runner| {
            let code = [0x40, 0xeb, 0xfd]; // Two-instruction loop, with checkpoint overshoot.
            prepare_cpu(&mut runner, &code).await;
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x100, &code, 2)
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
            assert!(matches!(
                runner.run_cpu_until(stopped).await,
                Either::Right(Ok(()))
            ));
            assert_eq!(runner.clock().now().ticks(), 90);
            assert_eq!(
                runner
                    .clock()
                    .duration_between(Timestamp::from_ticks(0), runner.clock().now()),
                Duration::from_millis(9)
            );
            runner.cpu().cpu_state().instruction_count()
        },
    );
    // At 750 instructions/s, four retired instructions cross the first 4 ms
    // boundary. The host wait includes that overshoot.
    expect_parked_timer(
        &mut armed,
        &host_time,
        host_time.at(Duration::from_nanos(5_333_334)),
    )
    .await;
    host.advance_by(Duration::from_millis(9));
    // A late wake starts a fresh burst; each four-instruction block pair is charged
    // at the host limit, independently of the nominal 10,000 Hz guest clock.
    expect_parked_timer(
        &mut armed,
        &host_time,
        host_time.at(Duration::from_nanos(14_333_334)),
    )
    .await;
    stop.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), Some(8));
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

async fn ready_service_during_pacing() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let (complete, completion) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(config()).with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: runner.clock().clone(),
                deadlines: vec![clock.deadline_after(start, Duration::from_millis(10))],
                observed: Vec::new(),
                exit: None,
                exit_after_waits: false,
            }));
            prepare_cpu(&mut runner, &[0x90]).await;
            let exit = runner.cpu().exit_handle();
            let completion = Rc::new(RefCell::new(Some(completion)));
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x100, async move || {
                let completion = completion.borrow_mut().take().unwrap();
                completion.await.unwrap();
                // The device timer is still pending on the host.
                exit.request();
            })])
            .await
            .unwrap();
            runner.connect(device.clone(), |_| {}).await;
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.clock().now(), start);
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
            assert!(device.borrow().observed.is_empty());
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(10)))
    );
    complete.send(()).unwrap();
    // A ready host service exits without waiting for the idle device timer.
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(host.clock().now(), host_time.at(Duration::ZERO));
    assert_eq!(host.next_deadline(), None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_limit_charges_actual_retirement_at_an_independent_rate() {
    pollster::block_on(busy_bursts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cpu_limit_charges_actual_retirement_at_an_independent_rate() {
    busy_bursts().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_ready_host_service_can_exit_during_an_idle_wait() {
    pollster::block_on(ready_service_during_pacing());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_ready_host_service_can_exit_during_an_idle_wait() {
    ready_service_during_pacing().await;
}
