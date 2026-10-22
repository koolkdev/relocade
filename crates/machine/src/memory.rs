mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, MachineConfig, Result};
use relocade_compiler::abi::Memory as WasmMemory;
use relocade_wasm_runtime::{WasmBackend, WasmSharedMemory};
use relocade_x86::abi;
use std::sync::Arc;

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
#[derive(Clone)]
pub struct Memory<S: WasmSharedMemory> {
    pub(crate) config: Arc<MachineConfig>,
    backing: S,
}

impl<S: WasmSharedMemory> Memory<S> {
    pub(crate) fn new<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
    ) -> Result<Self> {
        let memory = backend.create_memory(WasmMemory {
            minimum: config.memory_size().div_ceil(65536).max(1) as u32,
            ..abi::GUEST
        })?;
        let backing = backend.shared_memory(&memory).expect("shared guest memory");
        Self::with_backing(Arc::new(config), backing)
    }

    pub(crate) fn with_backing(config: Arc<MachineConfig>, backing: S) -> Result<Self> {
        if backing.byte_len() < config.memory_size() {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        Ok(Self { config, backing })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn backing(&self) -> &S {
        &self.backing
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&mut self) -> MemoryView<'_> {
        MemoryView::new(&self.config, &mut self.backing)
    }
}
