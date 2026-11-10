use relocade_machine::{
    worker::{Receiver, Thread},
    CpuConfig, CpuHandle, Machine, MachineConfig,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn prepared_cpus() {
    let map = PhysicalMemoryMap::new([(
        0..=0xfff,
        PhysicalMapping::Ram {
            backing_offset: 0x1000,
        },
    )])
    .unwrap();
    let mut machine = Machine::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
    )
    .unwrap();
    let first = machine.create_cpu(config).unwrap();
    let second = machine.create_cpu(config).unwrap();
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
