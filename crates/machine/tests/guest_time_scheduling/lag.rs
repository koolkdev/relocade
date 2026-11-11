use super::{host::*, *};
use futures_channel::oneshot;
use futures_lite::future::poll_once;
use futures_util::StreamExt;

async fn busy_lag_limit() {
    for (late_host, instructions) in [
        (Duration::from_millis(24), 28),
        (Duration::from_nanos(24_000_001), 8),
    ] {
        let (host, mut armed, source) = host();
        let host_time = host.clone();
        let (stop, stopped) = oneshot::channel();
        let setup = configuration();
        let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
        let prepared = machine.create_cpu(setup.cpu, None).unwrap();
        let worker = prepared.spawn_with_guest_time(
            move || config(source),
            async move |mut runner| {
                let clock = runner.clock().clone();
                let start = clock.now();
                let code = [0x40, 0xeb, 0xfd]; // INC AX; JMP 0100.
                prepare_cpu(&mut runner, &code, IoBus::default()).await;
                let block = Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x100, &code, 2)
                    .unwrap();
                let cpu = runner.cpu();
                cpu.install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        address: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
                let exit = runner.cpu().exit_handle();
                let ((), result) = join(
                    async move {
                        stopped.await.unwrap();
                        exit.request();
                    },
                    runner.run_cpu(),
                )
                .await;
                assert_eq!(result, ExecutionExit::Host(HostExit::Return));
                assert_eq!(runner.cpu().cpu_state().instruction_count(), instructions);
                assert_eq!(
                    runner.clock().now(),
                    clock.add(start, Duration::from_millis(instructions))
                );
            },
        );
        assert_eq!(
            armed.next().await,
            Some(host_time.at(Duration::from_millis(4)))
        );
        host.advance_by(late_host);
        // Guest time is 4 ms at the late wake. Exactly 20 ms of lag retains the
        // mapping; one nanosecond more discards all lag and permits only one slice.
        assert_eq!(
            armed.next().await,
            Some(host_time.at(late_host + Duration::from_millis(4)))
        );
        stop.send(()).unwrap();
        assert_eq!(worker.join().await.unwrap(), Some(()));
        assert_eq!(host.next_deadline(), None);
        assert_eq!(armed.next().await, None);
        machine.shutdown().await.unwrap();
    }
}

async fn pending_input_recovery() {
    let (host, mut armed, source) = host();
    let host_time = host.clone();
    let advancing_host = host.clone();
    let (arrive, arrival) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn_with_guest_time(
        move || config(source),
        async move |mut runner| {
            let clock = runner.clock().clone();
            let start = clock.now();
            let device = Rc::new(RefCell::new(Deadlines {
                clock: clock.clone(),
                deadlines: [2, 6]
                    .map(|ms| clock.add(start, Duration::from_millis(ms)))
                    .to_vec(),
                observed: Vec::new(),
                exit: None,
                exit_after_waits: false,
            }));
            let io = IoBus::new([runner.connect(device.clone(), [], []).await]);
            prepare_cpu(&mut runner, &[0x90], io).await;
            let exit = runner.cpu().exit_handle();
            let arrival = Rc::new(RefCell::new(Some(arrival)));
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x100, async move || {
                let arrival = arrival.borrow_mut().take().unwrap();
                arrival.await.unwrap();
                // Advance inside the service so both sync waits capture the old
                // mapping before the scheduler can recover at its next checkpoint.
                advancing_host.advance_by(Duration::from_millis(100));
                let mut input = clock.sync_to(
                    advancing_host.host_time(advancing_host.at(Duration::from_millis(90))),
                );
                let unpolled_input = clock.sync();
                let old_output = clock.sync_to_host(clock.add(start, Duration::from_millis(6)));
                let mut fixed = clock.sleep_until(clock.add(start, Duration::from_millis(6)));
                assert!(poll_once(&mut input).await.is_none());
                assert!(poll_once(&mut fixed).await.is_none());

                input.await;
                assert_eq!(clock.now(), clock.add(start, Duration::from_millis(4)));
                assert!(poll_once(unpolled_input).await.is_some());
                assert!(poll_once(old_output).await.is_some());
                assert!(poll_once(&mut fixed).await.is_none());
                fixed.await;
                assert_eq!(clock.now(), clock.add(start, Duration::from_millis(6)));
                clock.sync_to_host(clock.now()).await;
                exit.request();
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
            assert_eq!(
                runner.clock().now(),
                runner.clock().add(start, Duration::from_millis(6))
            );
            assert_eq!(
                device.borrow().observed,
                [2, 6].map(|ms| runner.clock().add(start, Duration::from_millis(ms)))
            );
        },
    );
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(4)))
    );
    arrive.send(()).unwrap();
    // The reset maps guest 4 ms to host 100 ms, so guest 6 ms now maps to 102 ms.
    assert_eq!(
        armed.next().await,
        Some(host_time.at(Duration::from_millis(102)))
    );
    host.advance_to(host_time.at(Duration::from_millis(102)));
    assert_eq!(worker.join().await.unwrap(), Some(()));
    assert_eq!(host.next_deadline(), None);
    assert_eq!(armed.next().await, None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn busy_execution_discards_lag_only_beyond_twenty_milliseconds() {
    pollster::block_on(busy_lag_limit());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn busy_execution_discards_lag_only_beyond_twenty_milliseconds() {
    busy_lag_limit().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn recovery_releases_pending_input_without_advancing_device_deadlines() {
    pollster::block_on(pending_input_recovery());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn recovery_releases_pending_input_without_advancing_device_deadlines() {
    pending_input_recovery().await;
}
