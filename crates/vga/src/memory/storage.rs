//! Physical VRAM with immutable 64-KiB allocation blocks. Consecutive packed
//! bytes select successive planes; the first four blocks are the VGA planes.
#[cfg(feature = "renderer")]
use super::DirtyPages;
use super::{PLANE_BYTES, VIDEO_MEMORY_BYTES};
use std::sync::Arc;

const BLOCKS: usize = VIDEO_MEMORY_BYTES as usize / PLANE_BYTES;

pub(super) struct Storage {
    blocks: [Arc<[u8]>; BLOCKS],
    // The existing VGA renderer observes only the first four physical planes.
    #[cfg(feature = "renderer")]
    changed: DirtyPages,
}

impl Default for Storage {
    fn default() -> Self {
        let zero: Arc<[u8]> = vec![0; PLANE_BYTES].into();
        Self {
            blocks: std::array::from_fn(|_| zero.clone()),
            #[cfg(feature = "renderer")]
            changed: DirtyPages::default(),
        }
    }
}

impl Storage {
    pub fn clear(&mut self) {
        *self = Self::default();
        // Clearing replaces the legacy planes too. Retained captures keep their
        // old storage, and the next VGA upload must see every cleared page.
        #[cfg(feature = "renderer")]
        for plane in 0..4 {
            self.changed.mark_plane(plane);
        }
    }

    /// Internal callers validate packed offsets against VIDEO_MEMORY_BYTES.
    fn location(address: u32) -> (usize, usize) {
        let address = address as usize;
        let block = address / (4 * PLANE_BYTES) * 4 + (address & 3);
        (block, (address / 4) % PLANE_BYTES)
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
        if block < 4 {
            self.changed.mark(block, offset);
        }
    }

    pub fn planes(&self) -> [Arc<[u8]>; 4] {
        std::array::from_fn(|plane| self.blocks[plane].clone())
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
