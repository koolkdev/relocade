//! Mutable physical VRAM; immutable snapshot blocks are prepared at capture.
use super::{DirtyPages, BLOCKS, BLOCK_BYTES};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

#[derive(Clone)]
pub(crate) struct MemorySnapshot {
    pub blocks: Arc<[Arc<[u8]>]>,
}

impl MemorySnapshot {
    pub fn read(&self, address: u32) -> Option<u8> {
        let address = address as usize;
        self.blocks
            .get(address / BLOCK_BYTES)
            .map(|bytes| bytes[address % BLOCK_BYTES])
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
    captured: RefCell<[Arc<[u8]>; BLOCKS]>,
    dirty: Cell<DirtyPages>,
    #[cfg(feature = "renderer")]
    changed: Cell<DirtyPages>,
}

impl Default for Storage {
    fn default() -> Self {
        let zero: Arc<[u8]> = vec![0; BLOCK_BYTES].into();
        Self {
            bytes: vec![0; BLOCKS * BLOCK_BYTES].into_boxed_slice(),
            captured: RefCell::new(std::array::from_fn(|_| zero.clone())),
            dirty: Cell::default(),
            #[cfg(feature = "renderer")]
            changed: Cell::default(),
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
            self.dirty.get_mut().mark_block(block);
        }
    }

    pub fn read(&self, address: u32) -> u8 {
        self.bytes[address as usize]
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let address = address as usize;
        self.bytes[address] = value;
        self.dirty
            .get_mut()
            .mark(address / BLOCK_BYTES, address % BLOCK_BYTES);
    }

    pub fn write_bytes(&mut self, address: u32, values: &[u8]) {
        let address = address as usize;
        self.bytes[address..address + values.len()].copy_from_slice(values);
        self.dirty.get_mut().mark_range(address, values.len());
    }

    /// Copy blocks containing written pages, even if their bytes are equal.
    /// Cache maintenance changes neither live bytes nor CPU access state.
    pub fn snapshot(&self, bytes: u32) -> MemorySnapshot {
        let count = (bytes as usize).div_ceil(BLOCK_BYTES);
        let mut captured = self.captured.borrow_mut();
        let mut dirty = self.dirty.get();
        #[cfg(feature = "renderer")]
        let mut changed = self.changed.get();
        for block in 0..count {
            let pages = dirty.take_block(block);
            if pages == 0 {
                continue;
            }
            let base = block * BLOCK_BYTES;
            captured[block] = Arc::from(&self.bytes[base..base + BLOCK_BYTES]);
            #[cfg(feature = "renderer")]
            changed.mark_pages(block, pages);
        }
        self.dirty.set(dirty);
        #[cfg(feature = "renderer")]
        self.changed.set(changed);
        MemorySnapshot {
            blocks: Arc::from(&captured[..count]),
        }
    }

    #[cfg(feature = "renderer")]
    pub fn changed_pages(&self) -> DirtyPages {
        self.changed.get()
    }

    #[cfg(feature = "renderer")]
    pub fn clear_changes(&mut self) {
        *self.changed.get_mut() = DirtyPages::default();
    }
}

#[cfg(test)]
mod tests;
