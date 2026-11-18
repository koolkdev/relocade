//! Wasm memory resources and scoped access for runtime execution.

use super::{ExecutionInterrupt, Result};
use crate::{CpuConfig, CpuStateAccess, Memory as GuestMemory};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{
    FunctionRef, Import, Memory, SharedMemory, WasmBackend, WasmMemoryAccess,
};
use relocade_x86::abi;

/// Retained CPU and guest storage, plus private mapping storage.
/// Retaining the guest handle lets the context borrow it independently of the Wasm runtime.
pub(super) struct RuntimeMemory {
    pub(crate) config: CpuConfig,
    pub(crate) context: Memory,
    /// Retained backing for execution controls whose cleanup outlives a Wasm call.
    pub(crate) context_backing: SharedMemory,
    pub(crate) cpu: CpuStateAccess,
    pub(crate) interrupt: ExecutionInterrupt,
    pub(crate) guest: GuestMemory,
    guest_import: Memory,
    pub(crate) mapping: Memory,
    pub(crate) code_revision: u64,
    code_write: FunctionRef<super::WasmRuntime, abi::CodeWriteSignature>,
}

impl RuntimeMemory {
    pub fn new(
        wasm: &mut super::WasmRuntime,
        guest: GuestMemory,
        config: CpuConfig,
        storage: SharedMemory,
    ) -> Result<Self> {
        guest.configuration().validate_for(config.cpu().mode())?;
        let context = wasm.import_shared_memory(&storage);
        let interrupt = ExecutionInterrupt::new(storage.clone());
        let cpu = CpuStateAccess::new(&config, storage.clone());
        let guest_import = wasm.import_shared_memory(guest.backing());
        let (code_revision, map) = guest.code_memory_map();
        let mapping_bytes = map.to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = wasm.create_memory(MemoryDeclaration {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        wasm.memory(&mapping).write(0, &mapping_bytes);
        let write_guest = guest.clone();
        let code_write = FunctionRef::new(
            &mut wasm.context(),
            move |_, (backing, bytes, value): (u32, u32, u64)| {
                write_guest
                    .view()
                    .write_backing(backing, &value.to_le_bytes()[..bytes as usize])
                    .expect("tracked RAM fits backing");
            },
        );
        Ok(Self {
            config,
            context,
            context_backing: storage,
            cpu,
            interrupt,
            guest,
            guest_import,
            mapping,
            code_revision,
            code_write,
        })
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }

    pub(super) fn refresh_code_pages(&mut self, wasm: &mut super::WasmRuntime) {
        let (revision, map) = self.guest.code_memory_map();
        let bytes = map.to_bytes();
        let mut mapping = wasm.memory(&self.mapping);
        // Keep CPU-local MMIO indices and the A20 mask. Transfer the table
        // in bulk rather than crossing the Wasm boundary for each flag word.
        let mut entries = [0; relocade_x86::PhysicalMemoryMap::A20_MASK_OFFSET];
        mapping.read(0, &mut entries);
        for (entry, updated) in entries.chunks_exact_mut(8).zip(bytes.chunks_exact(8)) {
            entry[..4].copy_from_slice(&updated[..4]);
        }
        mapping.write(0, &entries);
        self.code_revision = revision;
    }
    /// Binds CPU storage, guest memory, physical routing and tracked writes.
    pub fn imports<'a>(&'a self, wasm: &super::WasmRuntime) -> [Import<'a, super::WasmRuntime>; 4] {
        [
            wasm.bind(abi::EXECUTION_CONTEXT, &self.context),
            wasm.bind(abi::GUEST, &self.guest_import),
            wasm.bind(abi::PHYSICAL_MAP, &self.mapping),
            self.code_write.import(abi::WRITE_CODE),
        ]
    }
}
