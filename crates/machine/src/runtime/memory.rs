//! Wasm memory resources and scoped access for runtime execution.

use super::Result;
use crate::{CpuConfig, Memory as GuestMemory};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{Import, Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, ExecutionContext};
use std::mem::offset_of;

/// The runtime's Wasm memory bindings. This owner allocates private CPU and mapping
/// storage and imports the supplied guest backing without clearing it. The CPU
/// lends access to these bindings while the guest is stopped.
pub(super) struct RuntimeMemory {
    pub(crate) config: CpuConfig,
    pub(crate) guest: GuestMemory,
    pub(crate) context: Memory,
    guest_import: Memory,
    pub(crate) mapping: Memory,
}

impl RuntimeMemory {
    pub fn new(
        backend: &mut super::Backend,
        memory: &GuestMemory,
        config: CpuConfig,
    ) -> Result<Self> {
        let machine_config = memory.configuration();
        machine_config.validate_for(config.execution_target().profile)?;
        let context = backend.create_memory(MemoryDeclaration {
            maximum: Some(1),
            ..abi::EXECUTION_CONTEXT
        })?;
        backend.memory(&context).write(
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: config.execution_target().cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_import = backend.import_shared_memory(memory.backing());
        let mapping_bytes = machine_config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(MemoryDeclaration {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.memory(&mapping).write(0, &mapping_bytes);
        Ok(Self {
            config,
            guest: memory.clone(),
            context,
            guest_import,
            mapping,
        })
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }
    /// Binds the CPU, guest backing and physical translation memory.
    pub fn imports<'a, B: WasmBackend<Memory = Memory> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest_import),
            backend.bind(abi::PHYSICAL_MAP, &self.mapping),
        ]
    }

    pub(crate) fn set_instruction_target(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = Memory>,
        target: u64,
    ) {
        access.memory(&self.context).store(
            offset_of!(ExecutionContext, instruction_target) as u32,
            target,
        );
    }
}
