//! Captured display interpretation, independent of CPU port access state.
mod vga;
use crate::DisplayGeometry;
pub use vga::{VgaDisplayState, VgaGeometry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scanout {
    Vga(VgaDisplayState),
}

impl Scanout {
    pub fn geometry(&self) -> DisplayGeometry {
        match self {
            Self::Vga(state) => state.geometry().output,
        }
    }
}
