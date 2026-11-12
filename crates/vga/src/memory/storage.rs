//! Physical VRAM with immutable 64-KiB allocation blocks. Consecutive packed
//! bytes select successive planes; the first four blocks are the VGA planes.
#[cfg(feature = "renderer")]
use super::DirtyPages;
use super::{BLOCKS, BLOCK_BYTES};
use std::sync::Arc;

/// Complete immutable blocks covering a captured prefix of physical VRAM.
/// Sharing the block list also keeps capture clones and upload baselines cheap.
#[derive(Clone)]
pub(crate) struct MemorySnapshot {
    pub blocks: Arc<[Arc<[u8]>]>,
}

impl MemorySnapshot {
    pub fn read(&self, address: u32) -> Option<u8> {
        let (block, offset) = Storage::location(address);
        self.blocks.get(block).map(|bytes| bytes[offset])
    }

    pub fn same_blocks(&self, other: &Self) -> bool {
        self.blocks.len() == other.blocks.len()
            && self
                .blocks
                .iter()
                .zip(other.blocks.iter())
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

pub(super) struct Storage {
    blocks: [Arc<[u8]>; BLOCKS],
    #[cfg(feature = "renderer")]
    changed: DirtyPages,
}

impl Default for Storage {
    fn default() -> Self {
        let zero: Arc<[u8]> = vec![0; BLOCK_BYTES].into();
        Self {
            blocks: std::array::from_fn(|_| zero.clone()),
            #[cfg(feature = "renderer")]
            changed: DirtyPages::default(),
        }
    }
}

impl Storage {
    pub fn capacity(&self) -> u32 {
        (self.blocks.len() * BLOCK_BYTES) as u32
    }

    pub fn clear(&mut self) {
        *self = Self::default();
        // Retained captures keep their old storage. Every cleared block must be
        // visible to the next upload, including memory beyond the VGA planes.
        #[cfg(feature = "renderer")]
        for block in 0..BLOCKS {
            self.changed.mark_block(block);
        }
    }

    /// Internal callers validate packed offsets against VIDEO_MEMORY_BYTES.
    fn location(address: u32) -> (usize, usize) {
        let address = address as usize;
        let block = address / (4 * BLOCK_BYTES) * 4 + (address & 3);
        (block, (address / 4) % BLOCK_BYTES)
    }

    pub fn read(&self, address: u32) -> u8 {
        let (block, offset) = Self::location(address);
        self.blocks[block][offset]
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let (block, offset) = Self::location(address);
        if self.blocks[block][offset] == value {
            return;
        }
        // A retained snapshot copies at most one 64-KiB block on its first change.
        Arc::make_mut(&mut self.blocks[block])[offset] = value;
        #[cfg(feature = "renderer")]
        self.changed.mark(block, offset);
    }

    /// Capture complete interleaved groups through the requested physical byte
    /// extent. The caller supplies a layout bounded by physical VRAM.
    pub fn snapshot(&self, bytes: u32) -> MemorySnapshot {
        let count = (bytes as usize).div_ceil(4 * BLOCK_BYTES) * 4;
        MemorySnapshot {
            blocks: Arc::from(&self.blocks[..count]),
        }
    }

    #[cfg(feature = "renderer")]
    pub fn changed_pages(&self) -> DirtyPages {
        self.changed
    }

    #[cfg(feature = "renderer")]
    pub fn clear_changes(&mut self) {
        self.changed = DirtyPages::default();
    }
}

#[cfg(test)]
mod tests;
