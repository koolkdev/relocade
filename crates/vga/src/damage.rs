//! Conservative changes in physical VGA memory, independent of display layout.
use std::sync::Arc;

pub(crate) const PLANE_ROW_BYTES: usize = 256;
pub(crate) const PLANE_ROWS: usize = 256;
pub(crate) const PLANE_BYTES: usize = PLANE_ROW_BYTES * PLANE_ROWS;

/// A half-open rectangle in a physical plane arranged as 256 rows of 256 bytes.
/// These are memory coordinates, not screen pixels. A rectangle can include
/// unchanged bytes between writes; consumers may compare them before uploading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

impl MemoryRect {
    pub(crate) const FULL: Self = Self {
        left: 0,
        top: 0,
        right: PLANE_ROW_BYTES,
        bottom: PLANE_ROWS,
    };

    fn byte(address: usize) -> Self {
        let x = address % PLANE_ROW_BYTES;
        let y = address / PLANE_ROW_BYTES;
        Self {
            left: x,
            top: y,
            right: x + 1,
            bottom: y + 1,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// One accumulated rectangle per plane. Fonts and text therefore remain separate,
/// and graphics layouts can interpret the same physical changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryDamage {
    planes: [Option<MemoryRect>; 4],
}

impl MemoryDamage {
    pub(crate) const FULL: Self = Self {
        planes: [Some(MemoryRect::FULL); 4],
    };

    /// Changed bounds in one physical plane; `None` means no bytes changed.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> Option<MemoryRect> {
        self.planes[plane]
    }

    pub(crate) fn include_byte(&mut self, plane: usize, address: usize) {
        self.include(plane, MemoryRect::byte(address));
    }

    fn include(&mut self, plane: usize, rect: MemoryRect) {
        self.planes[plane] = Some(self.planes[plane].map_or(rect, |old| old.union(rect)));
    }

    pub(crate) fn union(&mut self, other: Self) {
        for (plane, rect) in other.planes.into_iter().enumerate() {
            if let Some(rect) = rect {
                self.include(plane, rect);
            }
        }
    }
}

/// The interval covered by accumulated memory damage. Public frame metadata is
/// deliberately separate: callers can relabel a capture without changing VRAM.
#[derive(Clone)]
pub(crate) struct CaptureChanges {
    pub source: Arc<()>,
    pub after: u64,
    pub sequence: u64,
    pub memory: MemoryDamage,
}

impl CaptureChanges {
    pub fn between(current: Option<&Self>, previous: Option<&Self>) -> MemoryDamage {
        if let (Some(current), Some(previous)) = (current, previous) {
            if Arc::ptr_eq(&current.source, &previous.source) {
                if current.sequence == previous.sequence {
                    return MemoryDamage::default();
                }
                if current.after <= previous.sequence && previous.sequence < current.sequence {
                    return current.memory;
                }
            }
        }
        // Immediate snapshots, another source or an older retained capture have
        // no applicable interval. Complete captured bytes still allow comparison.
        MemoryDamage::FULL
    }
}
