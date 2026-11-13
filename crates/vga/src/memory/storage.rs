//! Mutable physical VRAM; immutable snapshot blocks are prepared at capture.
use super::{DirtyPages, BLOCK_BYTES};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct MemorySnapshot {
    pub blocks: Arc<[Arc<[u8]>]>,
}

impl MemorySnapshot {
    pub fn bytes(&self) -> usize {
        self.blocks.len() * BLOCK_BYTES
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        let address = address as usize;
        self.blocks
            .get(address / BLOCK_BYTES)
            .map(|bytes| bytes[address % BLOCK_BYTES])
    }

    pub fn same_blocks(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
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

impl Storage {
    pub fn new(capacity: u32) -> Self {
        let capacity = capacity as usize;
        assert!(capacity != 0 && capacity.is_multiple_of(BLOCK_BYTES));
        Self {
            bytes: vec![0; capacity].into_boxed_slice(),
            dirty: DirtyPages::new(capacity / BLOCK_BYTES),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.bytes.len() as u32
    }

    pub fn clear(&mut self) {
        self.bytes.fill(0);
        for block in 0..self.bytes.len() / BLOCK_BYTES {
            self.dirty.mark_block(block);
        }
    }

    pub fn read(&self, address: u32) -> u8 {
        self.bytes[address as usize]
    }

    pub fn read_word(&self, address: u32) -> u32 {
        let address = address as usize;
        u32::from_le_bytes(self.bytes[address..address + 4].try_into().unwrap())
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let address = address as usize;
        self.bytes[address] = value;
        self.dirty
            .mark(address / BLOCK_BYTES, address % BLOCK_BYTES);
    }

    /// Replace an aligned physical word without reading its previous value.
    pub fn write_word(&mut self, address: u32, value: u32) {
        let address = address as usize;
        self.bytes[address..address + 4].copy_from_slice(&value.to_le_bytes());
        self.dirty
            .mark(address / BLOCK_BYTES, address % BLOCK_BYTES);
    }

    /// Replace selected bits in one aligned physical word. VGA's four plane
    /// bytes share that word; the caller has already applied its latch logic.
    pub fn write_masked_word(&mut self, address: u32, value: u32, mask: u32) {
        let address = address as usize;
        let word = &mut self.bytes[address..address + 4];
        let old = u32::from_le_bytes(word.try_into().unwrap());
        word.copy_from_slice(&((old & !mask) | (value & mask)).to_le_bytes());
        self.dirty
            .mark(address / BLOCK_BYTES, address % BLOCK_BYTES);
    }

    pub fn write_bytes(&mut self, address: u32, values: &[u8]) {
        let address = address as usize;
        self.bytes[address..address + values.len()].copy_from_slice(values);
        self.dirty.mark_range(address, values.len());
    }

    /// Reuse untouched blocks from the output owner's baseline. Inspection
    /// snapshots leave both that baseline and the dirty interval unchanged.
    pub fn snapshot(&self, bytes: u32, baseline: Option<&MemorySnapshot>) -> MemorySnapshot {
        assert!(bytes <= self.capacity(), "capture exceeds physical VRAM");
        let count = (bytes as usize).div_ceil(BLOCK_BYTES);
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
        self.dirty.clone()
    }

    /// The output owner starts a new interval after accepting a capture.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
}

#[cfg(test)]
mod tests;
