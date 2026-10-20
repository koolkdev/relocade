use crate::wasm::WasmSharedMemory;
use relocade_machine::{MachineConfig, MemoryError, ReadMemory};
use relocade_x86::PhysicalMemoryMap;
use std::sync::Arc;

/// Independently owned read access to the guest's fixed physical map and backing.
/// It exposes no CPU, host state or execution bookkeeping. Reads may overlap
/// execution and host callbacks; a multi-byte read is not an atomic snapshot.
#[derive(Clone)]
pub struct SharedMemory<M: WasmSharedMemory> {
    backing: M,
    map: Arc<PhysicalMemoryMap>,
    size: u64,
}

impl<M: WasmSharedMemory> SharedMemory<M> {
    /// Associates a shared backing with its machine's fixed map. The backing
    /// must contain at least `config.memory_size()` bytes, as with stopped
    /// machine storage. This also accepts a handle received by a browser worker.
    pub fn new(config: &MachineConfig, backing: M) -> Self {
        Self {
            backing,
            map: Arc::new(config.memory_map().clone()),
            size: config.memory_size(),
        }
    }

    /// Backend resource for sharing with another host thread or browser worker.
    /// Direct accesses to this handle use backing offsets, not guest addresses.
    pub fn backing(&self) -> &M {
        &self.backing
    }
}

impl<M: WasmSharedMemory> ReadMemory for SharedMemory<M> {
    fn memory_map(&self) -> &PhysicalMemoryMap {
        &self.map
    }

    fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        if u64::from(offset)
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(MemoryError::BackingRange);
        }
        self.backing.read(offset, bytes);
        Ok(())
    }
}
