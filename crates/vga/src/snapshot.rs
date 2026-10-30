//! Owned display inputs. Capturing these values never performs a CPU memory or
//! port access, so it leaves read latches and register access phases untouched.
use crate::Registers;
use std::sync::Arc;

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
}

impl DisplaySnapshot {
    /// One complete 64-KiB physical plane, including bytes outside the display.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> &[u8] {
        &self.planes[plane]
    }
}
