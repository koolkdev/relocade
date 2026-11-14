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
    code_state: Memory,
    write_code: FunctionRef<super::WasmRuntime, fn(u32, u8)>,
    code_revision: u64,
    pub(crate) mapping: Memory,
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
        let code_state = wasm.import_shared_memory(&guest.code_state);
        let writes = guest.clone();
        let write_code = FunctionRef::new(&mut wasm.context(), move |_, (offset, value)| {
            writes
                .view()
                .write_backing(offset, &[value])
                .expect("mapped instruction backing");
        });
        let mapping_bytes = guest.configuration().memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = wasm.create_memory(MemoryDeclaration {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        wasm.memory(&mapping).write(0, &mapping_bytes);
        Ok(Self {
            config,
            context,
            context_backing: storage,
            cpu,
            interrupt,
            guest,
            guest_import,
            code_state,
            write_code,
            code_revision: u64::MAX,
            mapping,
        })
    }

    pub(super) fn refresh_code_pages(&mut self, wasm: &mut super::WasmRuntime) {
        let Some(pages) = self.guest.code_pages(&mut self.code_revision) else {
            return;
        };
        let map = self.guest.configuration().memory_map();
        let original = map.to_bytes();
        let mut memory = wasm.memory(&self.mapping);
        for page in 0..relocade_x86::PhysicalMemoryMap::PAGE_COUNT {
            let address = page as u32 * relocade_x86::PhysicalMemoryMap::PAGE_BYTES;
            match map.get(address) {
                relocade_x86::PhysicalMapping::Ram { backing_offset }
                | relocade_x86::PhysicalMapping::Rom { backing_offset } => {
                    // Update the routed page after A20; MMIO payloads stay owned by the bus.
                    let offset = (map.bus_address(address) >> 12) * 8;
                    let mut kind = u32::from_le_bytes(
                        original[offset as usize..offset as usize + 4]
                            .try_into()
                            .unwrap(),
                    );
                    if pages.contains(&(backing_offset >> 12)) {
                        kind |= relocade_x86::PhysicalMemoryMap::CODE_PAGE;
                    }
                    memory.store(offset, kind);
                }
                _ => {}
            }
        }
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }
    /// Binds the CPU, guest backing and physical translation memory.
    pub fn imports<'a>(&'a self, wasm: &super::WasmRuntime) -> [Import<'a, super::WasmRuntime>; 5] {
        [
            wasm.bind(abi::EXECUTION_CONTEXT, &self.context),
            wasm.bind(abi::GUEST, &self.guest_import),
            wasm.bind(abi::PHYSICAL_MAP, &self.mapping),
            wasm.bind(abi::CODE_STATE, &self.code_state),
            self.write_code.import(abi::WRITE_CODE),
        ]
    }
}
