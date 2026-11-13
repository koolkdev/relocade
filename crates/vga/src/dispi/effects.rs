//! DISPI policy for the shared synchronous effects operation.
mod writes;

use crate::{
    effects::CardEffects, io::Io, mmio::MemoryDecoding, output::Output, ports::PortDecoding,
    DispiAdapter, PackedLayout, Timing,
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
        self.dispi.display_layout()
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

    fn update_memory_access(&mut self, decoding: &MemoryDecoding) {
        if let MemoryDecoding::Packed(window) = decoding {
            self.packed_window = *window;
        }
    }
}
