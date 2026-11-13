//! DISPI policy for the shared synchronous effects operation.
mod writes;

use crate::{
    effects::{CardEffects, VideoEffect},
    io::Io,
    mmio::MemoryDecoding,
    output::Output,
    ports::PortDecoding,
    DispiAdapter, PackedLayout, Scanout, Timing, VgaDisplayState,
};
use relocade_machine::Timestamp;

impl CardEffects for DispiAdapter {
    fn output(&mut self) -> &mut Output {
        &mut self.output
    }

    fn io(&self) -> Option<&Io> {
        self.io.as_ref()
    }

    fn advance_raster(&mut self, now: Timestamp) {
        DispiAdapter::advance_raster(self, now);
    }

    fn display_layout(&self) -> Option<PackedLayout> {
        match self.output.scanout {
            Scanout::Packed(layout) => Some(layout),
            Scanout::Vga(_) => None,
        }
    }

    fn update_scanout(&mut self, _effect: VideoEffect) {
        self.output.scanout = match self.dispi.display_layout() {
            Some(layout) => Scanout::Packed(layout),
            None => Scanout::Vga(VgaDisplayState::from_registers(&self.vga.registers)),
        };
    }

    fn timing(&self) -> Timing {
        match self.dispi.display_layout() {
            Some(_) => self.dispi.timing().expect("active DISPI timing"),
            None => Timing::vga(&self.vga.registers),
        }
    }

    fn port_decoding(&self) -> PortDecoding {
        PortDecoding::vga(&self.vga)
    }

    fn memory_decoding(&self) -> MemoryDecoding {
        DispiAdapter::memory_decoding(self)
    }
}
