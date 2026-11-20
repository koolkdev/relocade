use super::{host::*, *};
use futures_lite::future::poll_once;
use futures_util::StreamExt;

async fn local_output_sync() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(config()).with_host_clock(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            prepare_cpu(&mut runner, &[0x90]).await;
            let exit = runner.cpu().exit_handle();
            let retained = Rc::new(RefCell::new(None));
            let captured = retained.clone();
            let running_host = host.clone();
            let running_clock = clock.clone();
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x100, async move || {
                assert!(poll_once(running_clock.sync_to_host(start)).await.is_some());
                let sync = running_clock
                    .sync_to_host(running_clock.deadline_after(start, Duration::from_millis(1010)));
                let elapsed = running_clock
                    .sync_to_host(running_clock.deadline_after(start, Duration::from_millis(5)));
                assert_eq!(
                    running_host.next_deadline(),
                    None,
                    "construction does not arm a timer"
                );
                {
                    let mut cancelled = running_clock.sync_to_host(
                        running_clock.deadline_after(start, Duration::from_millis(20)),
                    );
                    assert!(poll_once(&mut cancelled).await.is_none());
                    assert_eq!(
                        running_host.next_deadline(),
                        Some(running_host.at(Duration::from_millis(20)))
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
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert!(
                poll_once(
                    clock
                        .sync_to_host(runner.clock().deadline_after(start, Duration::from_secs(2)))
                )
                .await
                .is_none(),
                "the live machine still times output after CPU exit"
            );
            host.advance_by(Duration::from_secs(1));
            let (mut sync, elapsed) = retained.borrow_mut().take().unwrap();
            assert!(poll_once(elapsed).await.is_some());
            assert!(poll_once(&mut sync).await.is_none());
            assert_eq!(
                host.next_deadline(),
                Some(host.at(Duration::from_millis(1010))),
                "the wait retains the host deadline captured during CPU execution"
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
        Some(host_time.at(Duration::from_millis(20)))
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_secs(2)))
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(1010)))
    );
    assert_eq!(
        armed.next().await,
        None,
        "elapsed waits must not register a timer"
    );
    machine.shutdown().await.unwrap();
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

async fn output_without_pacing() {
    let (host, _timers) = timer_service();
    let clock = host.clock();
    assert!(poll_once(clock.sync_to_host(clock.now())).await.is_some());

    let clock = relocade_machine::GuestTimeDriver::new(NonZeroU64::new(1000).unwrap()).clock();
    let future = clock.deadline_after(clock.now(), Duration::from_secs(1));
    assert!(poll_once(clock.sync_to_host(future)).await.is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unpaced_output_and_elapsed_host_output_complete_immediately() {
    pollster::block_on(output_without_pacing());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn unpaced_output_and_elapsed_host_output_complete_immediately() {
    output_without_pacing().await;
}

async fn output_from_an_unlimited_cpu() {
    let (host, _armed, source) = host();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = ExecutionWorker::spawn(
        prepared,
        SessionConfig::guest(relocade_machine::GuestTimeConfig::new(
            NonZeroU64::new(3_000).unwrap(),
        ))
        .with_host_clock(source),
        async move |mut runner| {
            prepare_cpu(&mut runner, &[0xe2, 0xfe]).await; // LOOP 0100.
            let cpu = runner.cpu();
            cpu.cpu_state().set_cx(64);
            let exit = cpu.exit_handle();
            cpu.install([Hook::host(0x102, async move || exit.request())])
                .await
                .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 64);

            let clock = runner.clock().clone();
            assert_eq!(clock.now(), Timestamp::from_ticks(0));
            assert_eq!(host.clock().now(), host.at(Duration::ZERO));
            assert!(poll_once(clock.sync_to_host(clock.now())).await.is_some());

            let at = clock.deadline_after(clock.now(), Duration::from_millis(10));
            let mut captured = clock.sync_to_host(at);
            assert!(poll_once(&mut captured).await.is_none());
            assert_eq!(
                host.next_deadline(),
                Some(host.at(Duration::from_millis(10)))
            );
            host.advance_by(Duration::from_secs(1));
            assert!(poll_once(captured).await.is_some());
            assert!(poll_once(clock.sync_to_host(at)).await.is_some());
            runner.wait_for(clock.sleep_until(at)).await;
            assert_eq!(clock.now(), Timestamp::from_ticks(3000));
            // Request the next 1/3000 s tick partway through its interval.
            host.advance_by(Duration::from_micros(200));
            let mut next = clock.sync_to_host(Timestamp::from_ticks(3001));
            assert!(poll_once(&mut next).await.is_none());
            assert_eq!(
                host.next_deadline(),
                Some(host.at(Duration::from_nanos(1_000_333_334)))
            );
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_time_and_presentation_follow_the_host_with_an_uncapped_cpu() {
    pollster::block_on(output_from_an_unlimited_cpu());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_time_and_presentation_follow_the_host_with_an_uncapped_cpu() {
    output_from_an_unlimited_cpu().await;
}
