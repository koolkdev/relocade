mod access;
pub(crate) use access::VramWindow;
mod storage;
pub(crate) use storage::MemorySnapshot;
use storage::Storage;

#[cfg(feature = "renderer")]
mod pages;
#[cfg(feature = "renderer")]
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

    pub fn write_physical(&mut self, address: u32, value: u8) {
        self.storage.write(address, value);
    }

    /// Reads each byte through the selected aperture; unmapped bytes return FF.
    pub fn read(&mut self, address: u32, bytes: u8, window: VramWindow) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let address = address + index as u32;
            *byte = window
                .memory_offset(address)
                .map_or(0xff, |offset| self.storage.read(offset));
        }
        u64::from_le_bytes(value)
    }

    pub fn write(&mut self, address: u32, bytes: u8, value: u64, window: VramWindow) {
        if bytes == 1 {
            if let Some(offset) = window.memory_offset(address) {
                self.storage.write(offset, value as u8);
            }
            return;
        }
        let skip = window
            .cpu_start
            .saturating_sub(address)
            .min(u32::from(bytes));
        let Some(offset) = window.memory_offset(address + skip) else {
            return;
        };
        let available = window.bytes - (address + skip - window.cpu_start);
        let count = (u32::from(bytes) - skip).min(available);
        self.storage.write_bytes(
            offset,
            &value.to_le_bytes()[skip as usize..(skip + count) as usize],
        );
    }

    /// Clears all physical VRAM.
    pub fn clear_vram(&mut self) {
        self.storage.clear();
    }

    pub fn snapshot(&self, bytes: u32) -> MemorySnapshot {
        self.storage.snapshot(bytes)
    }

    #[cfg(feature = "renderer")]
    pub fn changed_pages(&self) -> DirtyPages {
        self.storage.changed_pages()
    }

    #[cfg(feature = "renderer")]
    pub fn clear_changes(&mut self) {
        self.storage.clear_changes();
    }
}
