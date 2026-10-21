//! Wasm memory resources and scoped access for runtime execution.

mod shared;
pub use shared::SharedMemory;

use super::Result;
use crate::{CpuConfig, MachineConfig};
use relocade_compiler::abi::Memory;
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::{abi, ExecutionContext};
use std::sync::Arc;

/// The runtime's initialized Wasm memories. This owner allocates backend memory
/// from the machine configuration and supplies module bindings and scoped host
/// access. Architectural access policy belongs to `relocade_machine`.
#[derive(Clone)]
pub struct RuntimeMemory<M> {
    pub(crate) config: Arc<CpuConfig>,
    pub(crate) machine_config: Arc<MachineConfig>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
}

impl<M: Clone + Send + Sync + 'static> RuntimeMemory<M> {
    pub fn new<B: WasmBackend<Memory = M>>(
        backend: &mut B,
        machine_config: MachineConfig,
        config: CpuConfig,
    ) -> Result<Self> {
        machine_config.validate_for(config.execution_target().profile)?;
        let context = backend.create_memory(Memory {
            maximum: Some(1),
            ..abi::EXECUTION_CONTEXT
        })?;
        backend.write_memory(
            &context,
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: config.execution_target().cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_pages = machine_config.memory_size().div_ceil(65536).max(1) as u32;
        let guest = backend.create_memory(Memory {
            minimum: guest_pages,
            ..abi::GUEST
        })?;
        let mapping_bytes = machine_config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(Memory {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.write_memory(&mapping, 0, &mapping_bytes);
        Ok(Self {
            config: Arc::new(config),
            machine_config: Arc::new(machine_config),
            context,
            guest,
            mapping,
        })
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }
    pub fn context_memory(&self) -> &M {
        &self.context
    }
    pub fn guest_memory(&self) -> &M {
        &self.guest
    }

    /// Binds the CPU, guest backing and physical translation memory.
    pub fn imports<'a, B: WasmBackend<Memory = M> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest),
            backend.bind(abi::PHYSICAL_MAP, &self.mapping),
        ]
    }
}
