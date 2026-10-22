#![allow(dead_code)]
use relocade_machine::{CpuConfig, CpuContext, CpuView, MachineConfig, MemoryView};
use relocade_wasm_runtime::MemoryBytes;
use relocade_x86::ExecutionContext;

pub struct Storage {
    pub context: [u8; ExecutionContext::BYTE_LEN],
    bytes: Vec<u8>,
}

impl Storage {
    pub fn new(cpu: &CpuConfig, config: &MachineConfig) -> Self {
        Self {
            context: ExecutionContext {
                cpu: *cpu.cpu(),
                instruction_target: 998,
                cpu_model: cpu.execution_target().cpu_model,
            }
            .to_bytes(),
            bytes: vec![0; config.memory_size() as usize],
        }
    }

    pub fn view<'a>(&'a mut self, config: &'a Configuration) -> CpuContext<'a> {
        CpuContext::new(
            CpuView::new(&config.cpu, MemoryBytes::new(&mut self.context)),
            MemoryView::new(&config.machine, MemoryBytes::new(&mut self.bytes)),
        )
    }
}

/// Initial state for a test's machine and its first CPU.
pub struct Configuration {
    pub cpu: CpuConfig,
    pub machine: MachineConfig,
}

pub fn configuration(
    target: relocade_x86::ExecutionTarget,
    cpu: relocade_x86::CpuState,
    size: u64,
    map: relocade_x86::PhysicalMemoryMap,
) -> Result<Configuration, relocade_machine::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::new(target, cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}

pub async fn create_cpu<B: relocade_machine::RuntimeBackend>(
    backend: B,
    Configuration {
        cpu,
        machine: config,
    }: Configuration,
) -> relocade_machine::Result<relocade_machine::Cpu<B>> {
    relocade_machine::Cpu::new(backend, config, cpu).await
}
