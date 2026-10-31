use std::ops::Range;

const PAGE_BYTES: usize = 1024;

/// Physical write locations, independent of the display layout. Page bits bound
/// uploads; byte bounds let rendering restrict its work without scanning VRAM.
#[derive(Clone, Default)]
pub(crate) struct MemoryWrites {
    pages: [u64; 4],
    bounds: [Range<u32>; 4],
}

impl MemoryWrites {
    pub fn mark(&mut self, plane: usize, offset: usize) {
        self.pages[plane] |= 1 << (offset / PAGE_BYTES);
        let offset = offset as u32;
        let bounds = &mut self.bounds[plane];
        if bounds.start == bounds.end {
            *bounds = offset..offset + 1;
        } else {
            bounds.start = bounds.start.min(offset);
            bounds.end = bounds.end.max(offset + 1);
        }
    }

    pub fn bounds(&self, plane: usize) -> Range<usize> {
        let bounds = &self.bounds[plane];
        bounds.start as usize..bounds.end as usize
    }

    /// Consecutive dirty pages become one byte range within this plane.
    pub fn ranges(&self, plane: usize) -> impl Iterator<Item = Range<usize>> {
        let mut pages = self.pages[plane];
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
