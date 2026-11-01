//! Owned display inputs. Capturing these values never performs a CPU memory or
//! port access, so it leaves read latches and register access phases untouched.
#[cfg(feature = "renderer")]
use crate::memory::DirtyPages;
use crate::Registers;
use std::sync::Arc;

/// Changes since a known immutable memory version. Published captures accumulate
/// these until taken for upload. Retains plane references, not earlier captures,
/// so the latest snapshot cannot keep a capture history.
#[derive(Clone)]
#[cfg(feature = "renderer")]
pub(crate) struct MemoryChanges {
    pub baseline: [Arc<[u8]>; 4],
    pub pages: DirtyPages,
}

/// A complete display snapshot independent of the live device and other captures.
/// The four planes retain physical addresses across changes in display layout;
/// plane two includes all eight font banks. No memory is repacked into visible
/// cells, and no previous capture is required to interpret this snapshot.
/// Cloning shares immutable plane storage. Guest writes copy a shared plane before
/// changing it, so retained snapshots remain valid without copying VRAM at capture.
#[derive(Clone)]
pub struct DisplaySnapshot {
    /// A direct clone of the device's register banks, sampled with video memory.
    /// These settings apply to the whole image under the snapshot approximation.
    pub registers: Registers,
    pub blink_on: bool,
    pub(crate) planes: [Arc<[u8]>; 4],
    /// Optional transfer hints; complete planes remain authoritative. Keep these
    /// with their memory even when a caller replaces CapturedFrame::display.
    #[cfg(feature = "renderer")]
    pub(crate) changes: Option<MemoryChanges>,
}

impl DisplaySnapshot {
    // Conservative input comparison, not pixel equivalence. Plane identities
    // avoid scanning VRAM; register access state can cause harmless extra changes.
    pub(crate) fn same_inputs(&self, other: &Self) -> bool {
        self.blink_on == other.blink_on
            && self.registers == other.registers
            && self
                .planes
                .iter()
                .zip(&other.planes)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// One complete 64-KiB physical plane, including bytes outside the display.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> &[u8] {
        &self.planes[plane]
    }
}
