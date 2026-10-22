use crate::{MachineConfig, MemoryView};
use relocade_wasm_runtime::{MemoryBytes, WasmSharedMemory};

/// Shared guest allocations retained independently of CPU execution.
#[derive(Clone)]
pub(crate) struct MemoryResources<S: WasmSharedMemory> {
    pub(crate) backing: S,
}

impl<S: WasmSharedMemory> MemoryResources<S> {
    pub(crate) fn view<'a>(&'a self, config: &'a MachineConfig) -> MemoryView<'a> {
        MemoryView {
            config,
            backing: MemoryBytes::shared(&self.backing),
        }
    }
}
