use std::ops::Range;

const PAGE_BYTES: usize = 1024;

/// One bit for each physical 1-KiB page, independent of the display layout.
#[derive(Clone, Copy, Default)]
pub(crate) struct DirtyPages([u64; 4]);

impl DirtyPages {
    pub fn mark(&mut self, plane: usize, offset: usize) {
        self.0[plane] |= 1 << (offset / PAGE_BYTES);
    }

    /// Consecutive dirty pages become one byte range within this plane.
    pub fn ranges(self, plane: usize) -> impl Iterator<Item = Range<usize>> {
        let mut pages = self.0[plane];
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
