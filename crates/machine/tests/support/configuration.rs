use super::machine;
use machine::{CpuConfig, MachineConfig};

/// Initial state for a test's machine and its first CPU.
pub struct Configuration {
    pub cpu: CpuConfig,
    pub machine: MachineConfig,
}

pub fn configuration(
    target: relocade_x86::ExecutionTarget,
    cpu: relocade_x86::CpuState,
    size: u64,
    map: impl Into<machine::MemoryMap>,
) -> Result<Configuration, machine::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::new(target, cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}

pub fn with_descriptors(
    Configuration { cpu, machine }: Configuration,
    tables: relocade_x86::DescriptorTables,
    ldt: Option<u16>,
) -> Result<Configuration, machine::DescriptorError> {
    let image = machine::DescriptorMemory::new(tables, ldt)?;
    Ok(Configuration {
        cpu: cpu.with_descriptor_memory(&image)?,
        machine: machine.with_descriptor_memory(image)?,
    })
}

pub async fn create_cpu<
    T: 'static,
    B: machine::RuntimeBackend<State = machine::RuntimeState<T>>,
>(
    mut backend: B,
    Configuration {
        cpu,
        machine: config,
    }: Configuration,
) -> machine::Result<machine::Cpu<B>> {
    let machine = machine::Machine::new(&mut backend, config)?;
    machine.create_cpu(backend, cpu).await
}
