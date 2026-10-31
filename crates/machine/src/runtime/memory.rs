//! Wasm memory resources and scoped access for runtime execution.

use super::Result;
use crate::{CpuConfig, CpuStateAccess, Memory as GuestMemory};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{
    Import, Memory, SharedMemory, WasmBackend, WasmMemoryAccess, WasmSharedMemory,
};
use relocade_x86::{abi, ExecutionContext};

/// Retained CPU and guest storage, plus private mapping storage.
/// Retaining the guest handle lets the context borrow it independently of the backend.
pub(super) struct RuntimeMemory {
    pub(crate) config: CpuConfig,
    pub(crate) context: Memory,
    pub(crate) cpu: CpuStateAccess,
    pub(crate) guest: GuestMemory,
    guest_import: Memory,
    pub(crate) mapping: Memory,
}

impl RuntimeMemory {
    pub fn new(
        backend: &mut super::Backend,
        guest: GuestMemory,
        config: CpuConfig,
    ) -> Result<Self> {
        guest
            .configuration()
            .validate_for(config.execution_target().profile)?;
        let storage = SharedMemory::new(1, 1)?;
        storage.write(
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: config.execution_target().cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let context = backend.import_shared_memory(&storage);
        let cpu = CpuStateAccess::new(&config, storage);
        let guest_import = backend.import_shared_memory(guest.backing());
        let mapping_bytes = guest.configuration().memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(MemoryDeclaration {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.memory(&mapping).write(0, &mapping_bytes);
        Ok(Self {
            config,
            context,
            cpu,
            guest,
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
}
