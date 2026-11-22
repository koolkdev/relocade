use super::setup;
#[cfg(target_arch = "wasm32")]
use relocade_machine::worker::Thread;
use relocade_machine::{ConfigurationError, Cpu, CpuStorage, Error};

async fn pc_cpu_ownership() {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    let standalone = Cpu::new(memory.clone(), config, Default::default())
        .await
        .unwrap();
    let retained = standalone.cpu_state();
    retained.set_ax(71);
    assert!(matches!(
        machine.create_cpu(config, None),
        Err(Error::Configuration(ConfigurationError::CpuAlreadyAttached))
    ));
    assert!(machine.cpus().is_empty());
    drop(standalone);

    let prepared = machine.create_cpu(config, None).unwrap();
    assert!(matches!(
        machine.create_cpu(config, None),
        Err(Error::Configuration(ConfigurationError::CpuAlreadyAttached))
    ));
    assert_eq!(machine.cpus().len(), 1);
    assert!(matches!(
        CpuStorage::new()
            .unwrap()
            .into_cpu(memory.clone(), config, Default::default())
            .await,
        Err(Error::Configuration(ConfigurationError::CpuAlreadyAttached))
    ));

    // The execution owner can outlive the machine. Its attachment survives
    // transfer from the prepared handle to the running CPU.
    machine.shutdown().await.unwrap();
    let cpu = prepared.into_cpu(Default::default()).await.unwrap();
    assert!(matches!(
        Cpu::new(memory.clone(), config, Default::default()).await,
        Err(Error::Configuration(ConfigurationError::CpuAlreadyAttached))
    ));
    drop(cpu);
    let replacement = CpuStorage::new()
        .unwrap()
        .into_cpu(memory, config, Default::default())
        .await
        .unwrap();
    assert_eq!(
        replacement.cpu_state().ax(),
        config.cpu().registers.eax as u16
    );
    assert_eq!(retained.ax(), 71);
}

async fn machine_retains_cpu() {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    drop(machine.create_cpu(config, None).unwrap());
    assert!(matches!(
        Cpu::new(memory.clone(), config, Default::default()).await,
        Err(Error::Configuration(ConfigurationError::CpuAlreadyAttached))
    ));
    machine.shutdown().await.unwrap();
    Cpu::new(memory, config, Default::default()).await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_memory_admits_one_cpu_across_construction_paths() {
    pollster::block_on(pc_cpu_ownership());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_memory_admits_one_cpu_across_construction_paths() {
    Thread::new().run(pc_cpu_ownership).await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_owns_its_cpu_after_the_execution_handle_is_dropped() {
    pollster::block_on(machine_retains_cpu());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_owns_its_cpu_after_the_execution_handle_is_dropped() {
    Thread::new().run(machine_retains_cpu).await.unwrap();
}
