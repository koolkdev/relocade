use relocade_machine::{
    worker::{Receiver, Thread},
    CpuConfig, CpuHandle, Machine, MachineConfig,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
};
use std::{pin::pin, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn setup() -> (Machine, CpuConfig) {
    let map = PhysicalMemoryMap::new([(
        0..=0xfff,
        PhysicalMapping::Ram {
            backing_offset: 0x1000,
        },
    )])
    .unwrap();
    let machine = Machine::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    (machine, config)
}

async fn prepared_cpus() {
    let (mut machine, config) = setup();
    let first = machine.create_cpu(config, None).unwrap();
    let second = machine.create_cpu(config, None).unwrap();
    machine.cpus()[0].set_ax(0x1234);
    machine.cpus()[1].set_ax(11);
    // INC AX; MOV [0080],AX; JMP 0100. The requested interrupt ends the block.
    machine
        .memory()
        .view()
        .write(0x100, &[0x40, 0xa3, 0x80, 0, 0xeb, 0xfa])
        .unwrap();

    let interrupt = first.interrupt_handle();
    interrupt.request();
    let worker = Thread::new();
    let (mut input, task) = worker
        .listen(move |mut input: Receiver<CpuHandle>| async move {
            let prepared = input.recv().await.unwrap().unwrap();
            assert_eq!(prepared.memory().view().read_u16(0x80).unwrap(), 0);
            let mut cpu = prepared.into_cpu(Default::default()).await.unwrap();
            assert_eq!(
                cpu.cpu_state().execution_target(),
                config.execution_target()
            );
            assert_eq!(
                cpu.cpu_state().ax(),
                0x1234,
                "backend construction preserves host edits"
            );
            assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        })
        .unwrap();
    input.send(first).unwrap();
    task.await.unwrap();
    assert!(interrupt.take());
    assert_eq!(machine.cpus()[0].ax(), 0x1235);
    assert_eq!(machine.cpus()[0].instruction_count(), 3);
    assert_eq!(machine.cpus()[1].ax(), 11);
    assert_eq!(machine.cpus()[1].instruction_count(), 0);
    assert_eq!(machine.memory().view().read_u16(0x80).unwrap(), 0x1235);

    second.interrupt_handle().request();
    let (mut input, task) = worker
        .listen(|mut input: Receiver<CpuHandle>| async move {
            let prepared = input.recv().await.unwrap().unwrap();
            assert_eq!(prepared.memory().view().read_u16(0x80).unwrap(), 0x1235);
            let mut cpu = prepared.into_cpu(Default::default()).await.unwrap();
            assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        })
        .unwrap();
    input.send(second).unwrap();
    task.await.unwrap();
    assert_eq!(machine.cpus()[0].ax(), 0x1235);
    assert_eq!(machine.cpus()[1].ax(), 12);
    assert_eq!(machine.memory().view().read_u16(0x80).unwrap(), 12);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_cpus_keep_their_state_when_transferred_to_a_worker() {
    pollster::block_on(prepared_cpus());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_cpus_keep_their_state_when_transferred_to_a_worker() {
    prepared_cpus().await;
}

async fn session_shutdown() {
    let (mut machine, config) = setup();
    let prepared = machine.create_cpu(config, None).unwrap();
    machine.cpus()[0].set_ax(41);
    machine
        .memory()
        .view()
        .write(0x100, &[0x40, 0xeb, 0])
        .unwrap();
    let completed = prepared
        .run_until(std::future::pending::<()>(), async |mut runner| {
            assert!(runner.take_compiler().is_none());
            let clock = runner.clock().clone();
            let cpu = runner.cpu();
            assert_eq!(cpu.cpu_state().ax(), 41);
            let exit = cpu.exit_handle();
            cpu.install([relocade_machine::Hook::host(0x103, async move || {
                exit.request()
            })])
            .await
            .unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(relocade_x86::HostExit::Return)
            );
            let ax = runner.cpu().cpu_state().ax();
            drop(runner);
            // The callback and device task set remain alive after releasing Runner.
            clock.sleep(Duration::from_millis(1)).await;
            ax
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed, 42);
    assert_eq!(machine.cpus()[0].ax(), 42);

    let (armed, stop) = futures_channel::oneshot::channel();
    let prepared = machine.create_cpu(config, None).unwrap();
    let result = prepared
        .run_until(stop, async move |runner| {
            let mut wait = pin!(runner.clock().sleep(Duration::from_secs(3600)));
            assert!(futures_lite::future::poll_once(wait.as_mut())
                .await
                .is_none());
            armed.send(()).unwrap();
            wait.await;
        })
        .await
        .unwrap();
    assert_eq!(result, None);
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_completion_and_cancellation_stop_their_timer_workers() {
    pollster::block_on(session_shutdown());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_completion_and_cancellation_stop_their_timer_workers() {
    session_shutdown().await;
}
