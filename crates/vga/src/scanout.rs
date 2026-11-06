//! Captured display interpretation, independent of CPU port access state.
mod svga;
mod vga;
use crate::memory::VGA_MEMORY_BYTES;
use crate::DisplayGeometry;
pub use svga::SvgaLayout;
pub use vga::{VgaDisplayState, VgaGeometry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scanout {
    /// Text or graphics interpreted through captured VGA display registers.
    Vga(VgaDisplayState),
    /// Packed eight-bit indices interpreted through the shared DAC palette.
    Svga(SvgaLayout),
}

impl Scanout {
    pub(crate) fn memory_bytes(&self) -> u32 {
        match self {
            Self::Vga(_) => VGA_MEMORY_BYTES,
            Self::Svga(layout) => layout.memory_bytes(),
        }
    }

    pub fn geometry(&self) -> DisplayGeometry {
        match self {
            Self::Vga(state) => state.geometry().output,
            Self::Svga(layout) => layout.geometry(),
        }
    }
}
