use super::*;
use futures_channel::oneshot;
use relocade_machine::{ExecutionWorker, GuestTimeConfig, Hook};
use std::{num::NonZeroU64, sync::Mutex};

fn run_loop(cpu: CpuHandle) -> (ExecutionWorker<()>, oneshot::Receiver<()>) {
    let (entered, started) = oneshot::channel();
    let worker = cpu.spawn_with_guest_time(
        || GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()),
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
            runner.run_cpu().await;
            panic!("the loop ends only through worker cancellation");
        },
    );
    (worker, started)
}

async fn pause_workers() {
    let (mut machine, config) = setup();
    // Register-only loops avoid adding concurrent guest data writers.
    machine
        .memory()
        .view()
        .write(0x101, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let first = machine.create_cpu(config, None).unwrap();
    let second = machine.create_cpu(config, None).unwrap();
    let late = machine.create_cpu(config, None).unwrap();
    let _idle = machine.create_cpu(config, None).unwrap();
    let (first, first_started) = run_loop(first);
    let (second, second_started) = run_loop(second);
    first_started.await.unwrap();
    second_started.await.unwrap();

    let paused = machine.pause_cpus().await;
    let counts: Vec<_> = machine
        .cpus()
        .iter()
        .map(|cpu| cpu.instruction_count())
        .collect();
    let (late, mut late_started) = run_loop(late);
    futures_timer::Delay::new(Duration::from_millis(5)).await;
    assert_eq!(late_started.try_recv().unwrap(), None);
    for (cpu, count) in machine.cpus().iter().zip(&counts) {
        assert_eq!(cpu.instruction_count(), *count);
    }
    drop(paused);
    late_started.await.unwrap();
    futures_timer::Delay::new(Duration::from_millis(5)).await;
    let paused = machine.pause_cpus().await;
    for (cpu, count) in machine.cpus().iter().take(2).zip(counts) {
        assert!(cpu.instruction_count() > count);
    }
    // Stop stays live even when entry is closed. Dropped CPUs do not hold a pause.
    assert_eq!(first.stop().await.unwrap(), None);
    assert_eq!(second.stop().await.unwrap(), None);
    assert_eq!(late.stop().await.unwrap(), None);
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
fn machine_pause_stops_workers_and_blocks_late_entry_until_release() {
    pollster::block_on(pause_workers());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_pause_stops_workers_and_blocks_late_entry_until_release() {
    pause_workers().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_pause_gates_steps_and_pending_cpu_services() {
    pollster::block_on(paused_step_and_service());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_pause_gates_steps_and_pending_cpu_services() {
    paused_step_and_service().await;
}
