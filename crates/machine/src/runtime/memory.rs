//! Wasm memory resources and scoped access for runtime execution.

mod host_writes;
pub(super) use host_writes::{HostWritePage, HostWritePages};

use super::{ExecutionInterrupt, Result};
use crate::{memory::CpuBinding, CpuConfig, CpuStateAccess, Memory as GuestMemory};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{Import, Memory, SharedMemory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, MemoryBackend};
use std::sync::Arc;

/// Retained CPU and guest storage, plus the mapping import for this CPU.
/// Retaining the guest handle lets the context borrow it independently of the Wasm runtime.
pub(super) struct RuntimeMemory {
    config: Arc<CpuConfig>,
    pub(crate) context: Memory,
    pub(crate) cpu: CpuStateAccess,
    pub(crate) interrupt: ExecutionInterrupt,
    pub(crate) guest: GuestMemory,
    guest_import: Memory,
    pub(crate) mapping: Memory,
}

impl RuntimeMemory {
    pub fn new(
        wasm: &mut super::WasmRuntime,
        binding: CpuBinding,
        storage: SharedMemory,
    ) -> Result<Self> {
        let CpuBinding {
            memory: guest,
            config,
        } = binding;
        let context = wasm.import_shared_memory(&storage);
        let interrupt = ExecutionInterrupt::new(storage.clone());
        let cpu = CpuStateAccess::new(&config, storage);
        let guest_import = wasm.import_shared_memory(guest.backing());
        let mapping = if let Some(pages) = guest.page_table() {
            wasm.import_shared_memory(&pages.entries)
        } else {
            let mapping = wasm.create_memory(MemoryDeclaration {
                maximum: Some(abi::PHYSICAL_MAP.minimum),
                ..abi::PHYSICAL_MAP
            })?;
            wasm.memory(&mapping)
                .write(0, guest.configuration().memory_map().as_bytes());
            mapping
        };
        Ok(Self {
            config,
            context,
            cpu,
            interrupt,
            guest,
            guest_import,
            mapping,
        })
    }

    /// Binds CPU storage, guest memory and the configured address translation.
    pub fn imports<'a>(&'a self, wasm: &super::WasmRuntime) -> [Import<'a, super::WasmRuntime>; 3] {
        [
            wasm.bind(abi::EXECUTION_CONTEXT, &self.context),
            wasm.bind(abi::GUEST, &self.guest_import),
            wasm.bind(
                mapping_import(self.config.execution_target().memory_backend),
                &self.mapping,
            ),
        ]
    }
}

fn mapping_import(backend: MemoryBackend) -> MemoryDeclaration {
    match backend {
        MemoryBackend::PhysicalBus => abi::PHYSICAL_MAP,
        MemoryBackend::HostPageTable => abi::PAGE_TABLE,
    }
}
