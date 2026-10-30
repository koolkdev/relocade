mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, IoBus, MachineConfig, Result};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use std::sync::Arc;

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
/// Devices belong to the execution worker and are not retained by this handle.
#[derive(Clone)]
pub struct Memory {
    config: Arc<MachineConfig>,
    backing: SharedMemory,
}

impl Memory {
    /// Allocates zeroed guest backing with the supplied physical address policy.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let maximum = relocade_x86::abi::GUEST
            .maximum
            .expect("guest memory has a maximum");
        Self::with_backing(config, SharedMemory::new(minimum, maximum)?)
    }

    /// Associates shared backing with its guest address policy.
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

    /// Retains this memory and its local device bus as one memory-access handle.
    /// The resulting handle stays on the execution worker; raw memory can still
    /// be retained separately for sharing backing storage with other workers.
    pub fn with_io(self, io: IoBus) -> MemoryBus {
        MemoryBus { memory: self, io }
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    /// For MMIO, bind the execution worker's bus with [`Self::with_io`].
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view())
    }
}

/// Owned access to guest RAM, ROM and devices on one execution worker.
///
/// Clones retain the same [`Memory`] and [`IoBus`] independently of CPU execution.
/// Views borrow only this handle and may cross awaits. This does not retain a
/// backend or borrow a device between transfers. Use [`Memory`] separately when
/// sharing backing storage with another worker.
#[derive(Clone)]
pub struct MemoryBus {
    memory: Memory,
    io: IoBus,
}

impl MemoryBus {
    /// Borrows a live view with device routing already attached.
    pub fn view(&self) -> MemoryView<'_> {
        self.memory.view().with_io(&self.io)
    }
}
