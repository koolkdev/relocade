#![allow(dead_code)]
use relocade_machine::{CpuConfig, MachineConfig};
use relocade_x86::{CompilerConfig, EntryAssumptions, MemoryBackend};

pub fn pc_compiler_config() -> CompilerConfig {
    CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus)
}

/// Initial state for a test's machine and its first CPU.
pub struct Configuration {
    pub cpu: CpuConfig,
    pub machine: MachineConfig,
}

pub fn configuration(
    cpu: relocade_x86::CpuState,
    size: u64,
    map: relocade_x86::PhysicalMemoryMap,
) -> Result<Configuration, relocade_machine::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::pc(cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}

pub async fn create_cpu(
    Configuration {
        cpu,
        machine: config,
    }: Configuration,
) -> relocade_machine::Result<relocade_machine::Cpu> {
    let memory = relocade_machine::Memory::new(config)?;
    relocade_machine::Cpu::new(memory, cpu, Default::default()).await
}

mod execution;
#[allow(unused_imports)]
pub use execution::{checkpoint, guest_sleep, run_until, steps};
