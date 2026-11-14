//! Captured display interpretation, independent of CPU port access state.
mod configuration;
mod vga;
use crate::memory::VGA_MEMORY_BYTES;
use crate::{DisplayGeometry, PackedLayout};
pub(crate) use configuration::{DisplayConfiguration, PreparedScanout};
pub use vga::{VgaDisplayState, VgaGeometry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scanout {
    /// Text or graphics interpreted through captured VGA display registers.
    Vga(VgaDisplayState),
    /// Packed pixels interpreted using the captured layout and pixel format.
    Packed(PackedLayout),
}

impl Scanout {
    pub(crate) fn same_inputs(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Vga(left), Self::Vga(right)) => left.same_inputs(right),
            _ => self == other,
        }
    }

    pub(crate) fn memory_bytes(&self) -> u32 {
        match self {
            Self::Vga(_) => VGA_MEMORY_BYTES,
            Self::Packed(layout) => layout.memory_bytes(),
        }
    }

    pub fn geometry(&self) -> DisplayGeometry {
        match self {
            Self::Vga(state) => state.geometry().output,
            Self::Packed(layout) => layout.geometry(),
        }
    }
}
