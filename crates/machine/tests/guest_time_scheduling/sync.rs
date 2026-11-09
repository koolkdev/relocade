use super::{host::*, *};
use futures_channel::oneshot;
use futures_lite::future::poll_once;
use futures_util::{
    future::{select, Either},
    StreamExt,
};

async fn captured_deadline() {
    for timestamped in [false, true] {
        let (host, mut armed, source) = host();
        let host_start = host.clock().now();
        let advancing_host = host.clone();
        let (arrive, arrival) = oneshot::channel();
        let worker = ExecutionWorker::spawn_with_guest_time(
            move || config(source),
            async move |runner| {
                let clock = runner.clock().clone();
                let start = clock.now();
                assert!(
                    poll_once(clock.sync()).await.is_some(),
                    "no active CPU mapping yet"
                );
                assert!(poll_once(clock.sync_to_host(start)).await.is_some());
                let device = Rc::new(RefCell::new(Deadlines {
                    clock: clock.clone(),
                    deadlines: [2, 4, 6]
                        .map(|ms| start + Duration::from_millis(ms))
                        .to_vec(),
                    observed: Vec::new(),
                    exit: None,
                    exit_after_waits: false,
                }));
                let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
                let mut cpu = cpu(runner, &[0x90], io).await;
                let exit = cpu.exit_handle();
                let arrival = Rc::new(RefCell::new(Some(arrival)));
                cpu.install([Hook::host(0x100, async move || {
                    let arrival = arrival.borrow_mut().take().unwrap();
                    arrival.await.unwrap();
                    // Advance on the execution owner so it cannot idle past the
                    // sync deadline before this hook captures the wait.
                    advancing_host.advance_by(Duration::from_millis(7));
                    let sync = if timestamped {
                        clock.sync_to(host_start + Duration::from_millis(6))
                    } else {
                        clock.sync()
                    };
                    // Construction captures host 7 ms (or the supplied 6 ms).
                    // The host moves before the sync future receives its first poll.
                    advancing_host.advance_by(Duration::from_millis(2));
                    clock.sleep_until(start + Duration::from_millis(5)).await;
                    sync.await;
                    assert_eq!(
                        clock.now(),
                        start + Duration::from_millis(if timestamped { 6 } else { 7 })
                    );
                    exit.request();
                })])
                .await
                .unwrap();
                assert_eq!(
                    runner.run_cpu(&mut cpu).await,
                    ExecutionExit::Host(HostExit::Return)
                );
                assert_eq!(
                    device.borrow().observed,
                    [2, 4, 6].map(|ms| start + Duration::from_millis(ms))
                );
                assert_eq!(cpu.cpu_state().instruction_count(), 0);
            },
        );
        assert_eq!(
            armed.next().await,
            Some(host_start + Duration::from_millis(4))
        );
        arrive.send(()).unwrap();
        assert_eq!(worker.join().await.unwrap(), Some(()));
        assert_eq!(host.clock().now(), host_start + Duration::from_millis(9));
        assert_eq!(host.next_deadline(), None);
    }
}

async fn retained_sync_across_runs() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let advancing_host = host.clone();
    let (arrive, arrival) = oneshot::channel();
    let (pause, paused) = oneshot::channel();
    let (stopped, stop) = oneshot::channel();
    let (resume, resumed) = oneshot::channel();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: clock.clone(),
                deadlines: vec![start + Duration::from_millis(10)],
                observed: Vec::new(),
                exit: None,
                exit_after_waits: false,
            }));
            let io = IoBus::new([runner.connect(device, [], []).await]);
            let mut cpu = cpu(runner, &[0x90], io).await;
            let exit = cpu.exit_handle();
            let arrival = Rc::new(RefCell::new(Some(arrival)));
            let pause = Rc::new(RefCell::new(Some(pause)));
            cpu.install([Hook::host(0x100, async move || {
                let arrival = arrival.borrow_mut().take().unwrap();
                arrival.await.unwrap();
                // Capture and poll the sync before host progress can let the
                // scheduler advance the pending service into another slice.
                advancing_host.advance_by(Duration::from_millis(7));
                let mut sync = clock.sync_to(host_start + Duration::from_millis(7));
                assert!(poll_once(&mut sync).await.is_none());
                pause.borrow_mut().take().unwrap().send(()).unwrap();
                sync.await;
                assert_eq!(clock.now(), start + Duration::from_millis(7));
                exit.request();
            })])
            .await
            .unwrap();
            {
                match select(pin!(runner.run_cpu(&mut cpu)), pin!(paused)).await {
                    Either::Right((Ok(()), _)) => {}
                    _ => panic!("the run must be cancelled with sync pending"),
                }
            }
            assert_eq!(runner.clock().now(), start + Duration::from_millis(4));
            assert!(poll_once(runner.clock().sync_to_host(runner.clock().now()))
                .await
                .is_some());
            assert!(
                poll_once(runner.clock().sync()).await.is_some(),
                "a paused clock does not sync"
            );
            stopped.send(()).unwrap();
            resumed.await.unwrap();
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(4))
    );
    arrive.send(()).unwrap();
    stop.await.unwrap();
    assert_eq!(host.next_deadline(), None);
    host.advance_by(Duration::from_secs(1));
    resume.send(()).unwrap();
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(host.next_deadline(), None);
}

async fn clocks_without_host_mapping() {
    let host = relocade_clock_test_support::ManualClock::new().clock();
    let unpaced =
        relocade_machine::GuestTimeDriver::new(host.now(), NonZeroU64::new(1000).unwrap()).clock();
    for clock in [host, unpaced] {
        let output = clock.sync_to_host(clock.now());
        assert!(poll_once(output).await.is_some());
        assert!(poll_once(clock.sync()).await.is_some());
        assert!(poll_once(clock.sync_to(Instant::now())).await.is_some());
    }
}

async fn local_output_sync() {
    let (host, mut armed, source) = host();
    let host_start = host.clock().now();
    let worker = ExecutionWorker::spawn_with_guest_time(
        move || config(source),
        async move |runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let mut cpu = cpu(runner, &[0x90], IoBus::default()).await;
            let exit = cpu.exit_handle();
            let retained = Rc::new(RefCell::new(None));
            let captured = retained.clone();
            let running_host = host.clone();
            let running_clock = clock.clone();
            cpu.install([Hook::host(0x100, async move || {
                assert!(
                    poll_once(running_clock.sync_to_host(start - Duration::from_millis(1)))
                        .await
                        .is_some()
                );
                let sync = running_clock.sync_to_host(start + Duration::from_millis(1010));
                let elapsed = running_clock.sync_to_host(start + Duration::from_millis(5));
                assert_eq!(
                    running_host.next_deadline(),
                    None,
                    "construction does not arm a timer"
                );
                {
                    let mut cancelled =
                        running_clock.sync_to_host(start + Duration::from_millis(20));
                    assert!(poll_once(&mut cancelled).await.is_none());
                    assert_eq!(
                        running_host.next_deadline(),
                        Some(host_start + Duration::from_millis(20))
                    );
                }
                assert_eq!(
                    running_host.next_deadline(),
                    None,
                    "dropping the wait cancels its timer"
                );
                *captured.borrow_mut() = Some((sync, elapsed));
                exit.request();
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu(&mut cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert!(
                poll_once(clock.sync_to_host(start + Duration::from_secs(2)))
                    .await
                    .is_some(),
                "new waits on an inactive clock complete immediately"
            );
            host.advance_by(Duration::from_secs(1));
            let (mut sync, elapsed) = retained.borrow_mut().take().unwrap();
            assert!(poll_once(elapsed).await.is_some());
            assert!(poll_once(&mut sync).await.is_none());
            assert_eq!(
                host.next_deadline(),
                Some(host_start + Duration::from_millis(1010)),
                "the first poll after a pause retains the captured mapping"
            );
            host.advance_by(Duration::from_millis(9));
            assert!(poll_once(&mut sync).await.is_none());
            host.advance_by(Duration::from_millis(1));
            sync.await;
            assert_eq!(host.next_deadline(), None);
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(20))
    );
    assert_eq!(
        armed.next().await,
        Some(host_start + Duration::from_millis(1010))
    );
    assert_eq!(
        armed.next().await,
        None,
        "elapsed waits must not register a timer"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn output_sync_retains_its_host_deadline_on_the_execution_owner() {
    pollster::block_on(local_output_sync());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn output_sync_retains_its_host_deadline_on_the_execution_owner() {
    local_output_sync().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sync_captures_host_time_at_construction_and_services_earlier_deadlines() {
    pollster::block_on(captured_deadline());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn sync_captures_host_time_at_construction_and_services_earlier_deadlines() {
    captured_deadline().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_retained_sync_keeps_its_guest_deadline_across_cpu_run_cancellation() {
    pollster::block_on(retained_sync_across_runs());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_retained_sync_keeps_its_guest_deadline_across_cpu_run_cancellation() {
    retained_sync_across_runs().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clocks_without_a_host_mapping_sync_immediately() {
    pollster::block_on(clocks_without_host_mapping());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn clocks_without_a_host_mapping_sync_immediately() {
    clocks_without_host_mapping().await;
}
