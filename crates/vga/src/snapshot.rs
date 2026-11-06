//! Owned display inputs. Capturing these values never performs a CPU memory or
//! port access, so it leaves read latches and register access phases untouched.
#[cfg(feature = "renderer")]
use crate::memory::DirtyPages;
use crate::{memory::MemorySnapshot, DisplayGeometry, Scanout, VgaDisplayState};

/// Shared indexed-color display inputs, without CPU palette-transfer position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DacSnapshot {
    /// Six-bit RGB components in DAC address order.
    pub colors: [[u8; 3]; 256],
    pub mask: u8,
}

/// Changes since a known immutable memory version. Published captures accumulate
/// these until taken for upload. Retains memory references, not earlier captures,
/// so the latest snapshot cannot keep a capture history.
#[derive(Clone)]
#[cfg(feature = "renderer")]
pub(crate) struct MemoryChanges {
    pub baseline: MemorySnapshot,
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
    pub dac: DacSnapshot,
    pub(crate) scanout: Scanout,
    pub(crate) memory: MemorySnapshot,
    /// Optional transfer hints; complete captured memory remains authoritative.
    /// Keep these with their memory even when a caller replaces CapturedFrame::display.
    #[cfg(feature = "renderer")]
    pub(crate) changes: Option<MemoryChanges>,
}

impl DisplaySnapshot {
    // Conservative input comparison, not pixel equivalence. Block identities
    // avoid scanning VRAM; CPU port access state is not captured.
    pub(crate) fn same_inputs(&self, other: &Self) -> bool {
        self.scanout == other.scanout
            && self.dac == other.dac
            && self.memory.same_blocks(&other.memory)
    }

    pub fn geometry(&self) -> DisplayGeometry {
        self.scanout.geometry()
    }

    /// Display interpretation bound to this capture's physical memory extent.
    pub fn scanout(&self) -> &Scanout {
        &self.scanout
    }

    pub fn vga(&self) -> Option<&VgaDisplayState> {
        match &self.scanout {
            Scanout::Vga(state) => Some(state),
        }
    }

    pub fn vga_mut(&mut self) -> Option<&mut VgaDisplayState> {
        match &mut self.scanout {
            Scanout::Vga(state) => Some(state),
        }
    }

    /// One complete 64-KiB physical plane, including bytes outside the display.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> &[u8] {
        &self.memory.blocks[..4][plane]
    }
}
