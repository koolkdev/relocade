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
                    clock.deadline_after(start, Duration::from_millis(instructions))
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

async fn device_deadlines_during_recovery() {
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
                    .map(|ms| clock.deadline_after(start, Duration::from_millis(ms)))
                    .to_vec(),
                observed: Vec::new(),
                exit: None,
                exit_after_waits: false,
            }));
            runner.connect(device.clone(), |_| {}).await;
            prepare_cpu(&mut runner, &[0x90]).await;
            let exit = runner.cpu().exit_handle();
            let arrival = Rc::new(RefCell::new(Some(arrival)));
            let cpu = runner.cpu();
            cpu.install([Hook::host(0x100, async move || {
                let arrival = arrival.borrow_mut().take().unwrap();
                arrival.await.unwrap();
                // Capture an output wait before the scheduler recovers its pacing
                // correspondence at the next stopped boundary.
                advancing_host.advance_by(Duration::from_millis(100));
                let old_output =
                    clock.sync_to_host(clock.deadline_after(start, Duration::from_millis(6)));
                let mut fixed =
                    clock.sleep_until(clock.deadline_after(start, Duration::from_millis(6)));
                assert!(poll_once(&mut fixed).await.is_none());

                assert_eq!(
                    clock.now(),
                    clock.deadline_after(start, Duration::from_millis(4))
                );
                assert!(poll_once(old_output).await.is_some());
                assert!(poll_once(&mut fixed).await.is_none());
                fixed.await;
                assert_eq!(
                    clock.now(),
                    clock.deadline_after(start, Duration::from_millis(6))
                );
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
                runner
                    .clock()
                    .deadline_after(start, Duration::from_millis(6))
            );
            assert_eq!(
                device.borrow().observed,
                [2, 6].map(|ms| runner
                    .clock()
                    .deadline_after(start, Duration::from_millis(ms)))
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
fn recovery_preserves_device_deadlines_and_captured_output_waits() {
    pollster::block_on(device_deadlines_during_recovery());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn recovery_preserves_device_deadlines_and_captured_output_waits() {
    device_deadlines_during_recovery().await;
}
