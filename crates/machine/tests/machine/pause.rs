use super::*;
use futures_channel::oneshot;
use relocade_clock_test_support::ManualClock;
use relocade_machine::Hook;
use std::sync::Mutex;

async fn pause_worker() {
    let (mut machine, config) = setup();
    machine
        .memory()
        .view()
        .write(0x101, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let cpu = machine.create_cpu(config, None).unwrap();
    let paused = machine.pause_cpus().await;
    let (polled, first_poll) = oneshot::channel();
    let (entered, mut started) = oneshot::channel();
    let worker = ExecutionWorker::<()>::spawn(
        cpu,
        SessionConfig::host().with_host_clock(ManualClock::new()),
        async move |mut runner| {
            let state = runner.cpu().cpu_state();
            let entered = Mutex::new(Some(entered));
            runner
                .cpu()
                .install([Hook::host(0x100, async move || {
                    state.set_eip(0x101);
                    entered.lock().unwrap().take().unwrap().send(()).unwrap();
                })])
                .await
                .unwrap();
            let mut run = pin!(runner.run_cpu());
            assert!(futures_lite::future::poll_once(run.as_mut())
                .await
                .is_none());
            polled.send(()).unwrap();
            run.await;
            panic!("the loop ends only through worker cancellation");
        },
    );
    first_poll.await.unwrap();
    assert_eq!(started.try_recv().unwrap(), None);
    drop(paused);
    started.await.unwrap();

    let paused = machine.pause_cpus().await;
    let count = machine.cpus()[0].instruction_count();
    futures_timer::Delay::new(Duration::from_millis(5)).await;
    assert_eq!(machine.cpus()[0].instruction_count(), count);
    drop(paused);
    futures_timer::Delay::new(Duration::from_millis(5)).await;
    let paused = machine.pause_cpus().await;
    assert!(machine.cpus()[0].instruction_count() > count);
    // Cancellation stays available while entry is closed.
    assert_eq!(worker.stop().await.unwrap(), None);
    drop(paused);
    drop(machine.pause_cpus().await);
    machine.shutdown().await.unwrap();
}

async fn paused_step_and_service() {
    let (mut machine, config) = setup();
    machine.memory().view().write(0x100, &[0x40]).unwrap();
    let handle = machine.create_cpu(config, None).unwrap();
    let mut cpu = handle.into_cpu(Default::default()).await.unwrap();
    let paused = machine.pause_cpus().await;
    assert_eq!(cpu.step(), ExecutionExit::Interrupted);
    assert_eq!(cpu.cpu_state().instruction_count(), 0);
    drop(paused);
    cpu.interrupt_handle().take();
    assert_eq!(cpu.step(), ExecutionExit::StepComplete);
    assert_eq!(cpu.cpu_state().ax(), 1);

    let (reply, ready) = oneshot::channel();
    let ready = std::cell::RefCell::new(Some(ready));
    let state = cpu.cpu_state();
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x101, async move || {
        let ready = ready.borrow_mut().take().unwrap();
        ready.await.unwrap();
        state.set_ax(17);
        exit.request();
    })])
    .await
    .unwrap();
    {
        let mut run = pin!(cpu.run());
        assert!(futures_lite::future::poll_once(run.as_mut())
            .await
            .is_none());
    }
    let paused = machine.pause_cpus().await;
    reply.send(()).unwrap();
    {
        let mut run = pin!(cpu.run());
        assert!(futures_lite::future::poll_once(run.as_mut())
            .await
            .is_none());
    }
    assert_eq!(cpu.cpu_state().ax(), 1);
    drop(paused);
    assert_eq!(
        cpu.run().await,
        ExecutionExit::Host(relocade_x86::HostExit::Return)
    );
    assert_eq!(cpu.cpu_state().ax(), 17);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_pause_stops_the_worker_and_blocks_entry_until_release() {
    pollster::block_on(pause_worker());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_pause_stops_the_worker_and_blocks_entry_until_release() {
    pause_worker().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_pause_gates_steps_and_pending_cpu_services() {
    pollster::block_on(paused_step_and_service());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_pause_gates_steps_and_pending_cpu_services() {
    Thread::new().run(paused_step_and_service).await.unwrap();
}
