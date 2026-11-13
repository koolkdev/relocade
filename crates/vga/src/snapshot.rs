//! Owned display inputs. Capturing these values never performs a CPU memory or
//! port access, so it leaves read latches and register access phases untouched.
#[cfg(feature = "renderer")]
use crate::memory::DirtyPages;
use crate::{memory::MemorySnapshot, DisplayGeometry, Scanout, VgaDisplayState};
use std::ops::Range;

/// Blanked intervals in output pixels, independent of the image's memory layout.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Blanking {
    pub horizontal: [Range<u32>; 2],
    pub vertical: [Range<u32>; 2],
}

/// Shared indexed-color display inputs, without CPU palette-transfer position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DacSnapshot {
    /// Six-bit RGB components in DAC address order.
    pub colors: [[u8; 3]; 256],
    pub mask: u8,
}

/// Changes since a known immutable memory version. Pending captures accumulate
/// these until taken by the output task. Retains memory references, not earlier captures,
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
/// previous capture is required to interpret this snapshot. Capturing copies
/// written blocks from live VRAM and shares untouched immutable blocks. Cloning
/// shares those blocks; guest writes never modify captured storage.
#[derive(Clone)]
pub struct DisplaySnapshot {
    pub dac: DacSnapshot,
    /// Suppress output without changing the captured image geometry or memory.
    pub blanked: bool,
    pub(crate) blanking: Blanking,
    pub(crate) scanout: Scanout,
    pub(crate) memory: MemorySnapshot,
    /// Optional transfer hints; complete captured memory remains authoritative.
    /// A renderer with another baseline uploads complete changed blocks.
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
            changes.pages.merge(&pending.pages);
        }
    }

    // Conservative input comparison, not pixel equivalence. Only known CPU
    // controls are excluded; block identities avoid scanning VRAM.
    pub(crate) fn same_inputs(&self, other: &Self) -> bool {
        self.blanked == other.blanked
            && self.blanking == other.blanking
            && self.scanout.same_inputs(&other.scanout)
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
            Scanout::Packed(_) => None,
        }
    }

    pub fn vga_mut(&mut self) -> Option<&mut VgaDisplayState> {
        match &mut self.scanout {
            Scanout::Vga(state) => Some(state),
            Scanout::Packed(_) => None,
        }
    }

    /// A physical VRAM byte, or None outside the captured memory extent.
    pub fn vram_byte(&self, address: u32) -> Option<u8> {
        self.memory.read(address)
    }
}
