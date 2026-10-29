//! Owned display inputs. Capturing these values never performs a CPU memory or
//! port access, so it leaves read latches and register access phases untouched.
use crate::damage::{CaptureChanges, MemoryDamage};

/// Display register values sampled together with physical video memory.
/// Indices are the constants in [`crate::registers`]. CPU address indices, read
/// latches and DAC transfer phases are omitted because they do not drive output.
/// Under the frame-snapshot approximation these are the effective settings for
/// the entire image; they do not describe register changes within that frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRegisters {
    pub sequencer: [u8; 5],
    pub crtc: [u8; 0x19],
    pub graphics: [u8; 9],
    pub attribute: [u8; 0x15],
    pub miscellaneous: u8,
    /// Attribute Controller display enable (Palette Address Source).
    pub palette_address_source: bool,
    pub dac_mask: u8,
    /// Six-bit RGB components in DAC address order.
    pub palette: [[u8; 3]; 256],
}

/// A complete display snapshot independent of the live device and other captures.
/// The four planes retain physical addresses across changes in display layout;
/// plane two includes all eight font banks. No memory is repacked into visible
/// cells, and no previous capture is required to interpret this snapshot.
#[derive(Clone)]
pub struct DisplaySnapshot {
    pub registers: DisplayRegisters,
    pub blink_on: bool,
    pub(crate) planes: [Box<[u8]>; 4],
    pub(crate) changes: Option<CaptureChanges>,
}

impl DisplaySnapshot {
    /// Conservative physical memory changes since another snapshot. Pending
    /// refreshes accumulate changes until observed. If that interval does not
    /// cover `previous`, all planes are returned for comparison. Register and
    /// palette differences are separate and may change which memory is visible.
    pub fn memory_changes_since(&self, previous: &Self) -> MemoryDamage {
        CaptureChanges::between(self.changes.as_ref(), previous.changes.as_ref())
    }

    /// One complete 64-KiB physical plane, including bytes outside the display.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> &[u8] {
        &self.planes[plane]
    }
}
