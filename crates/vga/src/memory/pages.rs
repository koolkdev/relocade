use super::BLOCKS;
use std::ops::Range;

const PAGE_BYTES: usize = 1024;

/// One bit for each physical 1-KiB page, independent of the display layout.
#[derive(Clone, Copy)]
pub(crate) struct DirtyPages([u64; BLOCKS]);

impl Default for DirtyPages {
    fn default() -> Self {
        Self([0; BLOCKS])
    }
}

impl DirtyPages {
    pub fn merge(&mut self, other: Self) {
        for (pages, pending) in self.0.iter_mut().zip(other.0) {
            *pages |= pending;
        }
    }

    pub fn mark(&mut self, block: usize, offset: usize) {
        self.0[block] |= 1 << (offset / PAGE_BYTES);
    }

    pub fn mark_block(&mut self, block: usize) {
        self.0[block] = u64::MAX;
    }

    /// Consecutive dirty pages become one byte range within this block.
    pub fn ranges(self, block: usize) -> impl Iterator<Item = Range<usize>> {
        let mut pages = self.0[block];
        std::iter::from_fn(move || {
            if pages == 0 {
                return None;
            }
            let start = pages.trailing_zeros();
            let end = start + (pages >> start).trailing_ones();
            pages = if end == 64 {
                0
            } else {
                pages & (u64::MAX << end)
            };
            Some(start as usize * PAGE_BYTES..end as usize * PAGE_BYTES)
        })
    }
}
