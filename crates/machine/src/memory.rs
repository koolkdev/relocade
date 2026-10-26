mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, IoBus, MachineConfig, Result};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use std::sync::Arc;

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
/// The retained I/O bus connects mapped device accesses and CPU port instructions.
#[derive(Clone)]
pub struct Memory {
    config: Arc<MachineConfig>,
    backing: SharedMemory,
    io: IoBus,
}

impl Memory {
    /// Allocates zeroed guest backing and retains the machine's device connections.
    /// Use an empty [`IoBus`] for machines without devices.
    pub fn new(config: MachineConfig, io: IoBus) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let maximum = relocade_x86::abi::GUEST
            .maximum
            .expect("guest memory has a maximum");
        Self::with_backing(config, SharedMemory::new(minimum, maximum)?, io)
    }

    /// Associates shared backing with its guest address policy and device bus.
    /// Worker CPUs sharing devices must also retain the same bus, not just RAM.
    pub fn with_backing(config: MachineConfig, backing: SharedMemory, io: IoBus) -> Result<Self> {
        if backing.byte_len() < config.memory_size() {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        Ok(Self {
            config: Arc::new(config),
            backing,
            io,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn backing(&self) -> &SharedMemory {
        &self.backing
    }

    pub(crate) fn io(&self) -> &IoBus {
        &self.io
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view(), &self.io)
    }
}
