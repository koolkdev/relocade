mod access;
pub(crate) use access::{VramView, VramWindow};
mod storage;
pub(crate) use storage::MemorySnapshot;
use storage::Storage;

mod pages;
pub(crate) use pages::DirtyPages;

pub(crate) const BLOCK_BYTES: usize = 65536;

pub(crate) const VGA_MEMORY_BYTES: u32 = 4 * BLOCK_BYTES as u32;

/// Physical video memory, independent of register protocols and CPU read latches.
pub(super) struct VideoMemory {
    storage: Storage,
}

impl VideoMemory {
    pub fn new(capacity: u32) -> Self {
        Self {
            storage: Storage::new(capacity),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.storage.capacity()
    }

    /// Native video operations address the same physical bytes as the CPU aperture,
    /// without loading the CPU's read latches or applying its write registers.
    pub fn read_physical(&self, address: u32) -> u8 {
        self.storage.read(address)
    }

    pub fn write_physical(&mut self, address: u32, value: u8) {
        self.storage.write(address, value);
    }

    /// Clears all physical VRAM.
    pub fn clear_vram(&mut self) {
        self.storage.clear();
    }

    pub fn snapshot(&self, bytes: u32, baseline: Option<&MemorySnapshot>) -> MemorySnapshot {
        self.storage.snapshot(bytes, baseline)
    }

    #[cfg(feature = "renderer")]
    pub fn dirty_pages(&self) -> DirtyPages {
        self.storage.dirty_pages()
    }

    pub fn clear_dirty(&mut self) {
        self.storage.clear_dirty();
    }
}
