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
/// Retains physical memory covering the active scanout, including complete VGA
/// planes and font banks. No memory is repacked into visible pixels, and no
/// previous capture is required to interpret this snapshot. Cloning shares
/// immutable blocks; guest writes copy a shared block before changing it.
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
    /// Preserve upload hints when an intermediate capture was not consumed.
    #[cfg(feature = "renderer")]
    pub(crate) fn merge_changes(&mut self, previous: &Self) {
        if let Some(pending) = &previous.changes {
            let changes = self
                .changes
                .as_mut()
                .expect("later captures have a memory baseline");
            changes.baseline.clone_from(&pending.baseline);
            changes.pages.merge(pending.pages);
        }
    }

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
            Scanout::Svga(_) => None,
        }
    }

    pub fn vga_mut(&mut self) -> Option<&mut VgaDisplayState> {
        match &mut self.scanout {
            Scanout::Vga(state) => Some(state),
            Scanout::Svga(_) => None,
        }
    }

    /// A physical packed-address byte, or None outside the captured memory extent.
    pub fn vram_byte(&self, address: u32) -> Option<u8> {
        self.memory.read(address)
    }

    /// One complete 64-KiB physical plane, including bytes outside the display.
    /// Panics when `plane` is outside 0–3.
    pub fn plane(&self, plane: usize) -> &[u8] {
        &self.memory.blocks[..4][plane]
    }
}
