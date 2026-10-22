mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, MachineConfig, Result};
use relocade_wasm_runtime::{MemoryBytes, WasmSharedMemory};
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
    /// Associates shared backing with its guest address policy and capacity.
    pub fn with_backing(config: MachineConfig, backing: S) -> Result<Self> {
        if backing.byte_len() < config.memory_size() {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        Ok(Self {
            config: Arc::new(config),
            backing,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn backing(&self) -> &S {
        &self.backing
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&mut self) -> MemoryView<'_> {
        MemoryView::new(&self.config, MemoryBytes::shared(&self.backing))
    }
}
