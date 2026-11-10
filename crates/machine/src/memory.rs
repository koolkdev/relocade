mod resolve;
mod view;
pub use relocade_x86::real_mode::FarPointer;
pub use view::MemoryView;

use crate::{IoBus, MachineConfig, Result};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use std::{fmt, sync::Arc};

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
/// Devices belong to the execution worker and are not retained by this handle.
#[derive(Clone, crate::worker::Transfer)]
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
        let backing = SharedMemory::new(minimum, maximum)?;
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

    /// Retains this memory and its execution worker's device bus together.
    /// Keep raw [`Memory`] handles for sharing backing storage between workers.
    pub fn with_io(self, io: IoBus) -> MemoryBus {
        MemoryBus { memory: self, io }
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    /// For MMIO, retain device routing with [`Self::with_io`], or borrow a bus
    /// for this view with [`MemoryView::with_io`].
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view())
    }
}

/// Retained access to guest memory and devices on one execution worker.
///
/// Clones share backing bytes and device connections independently of a CPU.
/// Views borrow this handle and may cross awaits. Devices are borrowed only
/// for individual transfers. This handle does not retain a Wasm backend or
/// move device state between workers.
#[derive(Clone)]
pub struct MemoryBus {
    memory: Memory,
    io: IoBus,
}

impl MemoryBus {
    /// Borrows a live memory view with its device routing already attached.
    pub fn view(&self) -> MemoryView<'_> {
        self.memory.view().with_io(&self.io)
    }

    /// The same device connections used by this handle's memory views.
    pub fn io(&self) -> &IoBus {
        &self.io
    }
}

fn range_fits(address: u32, len: usize, limit: u64) -> bool {
    u64::from(address)
        .checked_add(len as u64)
        .is_some_and(|end| end <= limit)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    BackingRange,
    SegmentRange,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
            Self::SegmentRange => "memory access exceeds the segment limit",
        })
    }
}

impl std::error::Error for MemoryError {}
