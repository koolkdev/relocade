mod access;
pub(crate) use access::VramWindow;
mod storage;
pub(crate) use storage::MemorySnapshot;
use storage::Storage;

mod pages;
pub(crate) use pages::DirtyPages;

pub(crate) const BLOCK_BYTES: usize = 65536;

/// Physical video memory shared by legacy VGA planes and packed SVGA addressing.
pub const VIDEO_MEMORY_BYTES: u32 = 4 * 1024 * 1024;
pub(crate) const BLOCKS: usize = VIDEO_MEMORY_BYTES as usize / BLOCK_BYTES;
pub(crate) const VGA_MEMORY_BYTES: u32 = 4 * BLOCK_BYTES as u32;

/// Physical video memory, independent of register protocols and CPU read latches.
#[derive(Default)]
pub(super) struct VideoMemory {
    storage: Storage,
}

impl VideoMemory {
    pub fn capacity(&self) -> u32 {
        self.storage.capacity()
    }

    /// Native video operations address the same physical bytes as the CPU aperture,
    /// without loading the CPU's read latches or applying its write registers.
    pub fn read_physical(&self, address: u32) -> u8 {
        self.storage.read(address)
    }

    pub fn read_physical_word(&self, address: u32) -> u32 {
        self.storage.read_word(address)
    }

    pub fn write_physical(&mut self, address: u32, value: u8) {
        self.storage.write(address, value);
    }

    pub fn write_physical_word(&mut self, address: u32, value: u32) {
        self.storage.write_word(address, value);
    }

    pub fn write_physical_masked_word(&mut self, address: u32, value: u32, mask: u32) {
        self.storage.write_masked_word(address, value, mask);
    }

    pub fn write_physical_bytes(&mut self, address: u32, values: &[u8]) {
        self.storage.write_bytes(address, values);
    }

    pub fn physical_bytes_mut(&mut self, address: u32, bytes: usize) -> &mut [u8] {
        self.storage.bytes_mut(address, bytes)
    }

    /// The bus bounds the complete transfer to this validated window.
    #[inline(always)]
    pub fn read(&mut self, address: u32, bytes: u8, window: VramWindow) -> u64 {
        let mut value = [0; 8];
        let offset = window.memory_offset(address);
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.storage.read(offset + index as u32);
        }
        u64::from_le_bytes(value)
    }

    #[inline(always)]
    pub fn write(&mut self, address: u32, bytes: u8, value: u64, window: VramWindow) {
        let offset = window.memory_offset(address);
        if bytes == 1 {
            self.storage.write(offset, value as u8);
        } else {
            self.storage
                .write_bytes(offset, &value.to_le_bytes()[..usize::from(bytes)]);
        }
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
