#![allow(dead_code)]
use crate as machine;
use crate::{CpuContext, CpuView, MemoryView};
use relocade_wasm_runtime::MemoryBytes;
use relocade_x86::ExecutionContext;

#[path = "../../tests/support/configuration.rs"]
mod configuration;
#[allow(unused_imports)]
pub use configuration::*;

pub struct Fixture {
    pub context: [u8; ExecutionContext::BYTE_LEN],
    backing: Vec<u8>,
    mapping: Vec<u8>,
    revision: [u8; 8],
    descriptors: Vec<u8>,
}

impl Fixture {
    pub fn new(cpu: &crate::CpuConfig, config: &crate::MachineConfig) -> Self {
        Self {
            context: ExecutionContext {
                cpu: *cpu.cpu(),
                instruction_target: 998,
                cpu_model: cpu.execution_target().cpu_model,
            }
            .to_bytes(),
            backing: vec![0; config.memory_size() as usize],
            mapping: config.memory_map().to_bytes(),
            revision: [0; 8],
            descriptors: config.descriptor_memory().map_or_else(
                || vec![0; crate::DescriptorMemory::BYTE_LEN as usize],
                |image| image.to_bytes().to_vec(),
            ),
        }
    }

    pub fn view<'a>(&'a mut self, config: &'a Configuration) -> CpuContext<'a> {
        CpuContext::new(
            CpuView::new(&config.cpu, MemoryBytes::new(&mut self.context)),
            MemoryView {
                config: &config.machine,
                backing: MemoryBytes::new(&mut self.backing),
                mapping: matches!(config.machine.memory_map(), crate::MemoryMap::Paged(_))
                    .then(|| MemoryBytes::new(&mut self.mapping)),
                revision: MemoryBytes::new(&mut self.revision),
                descriptors: MemoryBytes::new(&mut self.descriptors),
            },
        )
    }
}
