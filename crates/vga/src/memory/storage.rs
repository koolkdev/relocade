//! Mutable physical VRAM; immutable snapshot blocks are prepared at capture.
use super::{DirtyPages, BLOCKS, BLOCK_BYTES};
use std::sync::Arc;

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
    bytes: Box<[u8]>,
    dirty: DirtyPages,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            bytes: vec![0; BLOCKS * BLOCK_BYTES].into_boxed_slice(),
            dirty: DirtyPages::default(),
        }
    }
}

impl Storage {
    pub fn capacity(&self) -> u32 {
        self.bytes.len() as u32
    }

    pub fn clear(&mut self) {
        self.bytes.fill(0);
        for block in 0..BLOCKS {
            self.dirty.mark_block(block);
        }
    }

    /// Translate a physical address into the existing plane storage.
    fn location(address: u32) -> (usize, usize) {
        let address = address as usize;
        let block = address / (4 * BLOCK_BYTES) * 4 + (address & 3);
        (block, (address / 4) % BLOCK_BYTES)
    }

    pub fn read(&self, address: u32) -> u8 {
        let (block, offset) = Self::location(address);
        self.bytes[block * BLOCK_BYTES + offset]
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let (block, offset) = Self::location(address);
        self.bytes[block * BLOCK_BYTES + offset] = value;
        self.dirty.mark(block, offset);
    }

    /// Reuse untouched blocks from the output owner's baseline. Inspection
    /// snapshots leave both that baseline and the dirty interval unchanged.
    pub fn snapshot(&self, bytes: u32, baseline: Option<&MemorySnapshot>) -> MemorySnapshot {
        let count = (bytes as usize).div_ceil(4 * BLOCK_BYTES) * 4;
        let mut blocks = Vec::with_capacity(count);
        for block in 0..count {
            let previous = baseline.and_then(|memory| memory.blocks.get(block));
            blocks.push(match previous {
                Some(bytes) if !self.dirty.contains_block(block) => bytes.clone(),
                _ => {
                    let base = block * BLOCK_BYTES;
                    Arc::from(&self.bytes[base..base + BLOCK_BYTES])
                }
            });
        }
        MemorySnapshot {
            blocks: blocks.into(),
        }
    }

    #[cfg(feature = "renderer")]
    pub fn dirty_pages(&self) -> DirtyPages {
        self.dirty
    }

    /// The output owner starts a new interval after accepting a capture.
    pub fn clear_dirty(&mut self) {
        self.dirty = DirtyPages::default();
    }
}

#[cfg(test)]
mod tests;
