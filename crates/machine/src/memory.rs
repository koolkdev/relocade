mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, MachineConfig, Result};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use std::sync::Arc;

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
#[derive(Clone)]
pub struct Memory {
    config: Arc<MachineConfig>,
    backing: SharedMemory,
}

impl Memory {
    /// Allocates zeroed guest memory before constructing a CPU.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let maximum = relocade_x86::abi::GUEST
            .maximum
            .expect("guest memory has a maximum");
        Self::with_backing(config, SharedMemory::new(minimum, maximum)?)
    }

    /// Associates shared backing with its guest address policy and capacity.
    pub fn with_backing(config: MachineConfig, backing: SharedMemory) -> Result<Self> {
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
    pub fn backing(&self) -> &SharedMemory {
        &self.backing
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view())
    }
}
