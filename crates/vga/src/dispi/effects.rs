//! DISPI policy for the shared synchronous effects operation.
mod writes;

use super::scanout;
use crate::{
    effects::CardEffects, io::Io, mmio::MemoryDecoding, ports::PortDecoding,
    scanout::DisplayConfiguration, DispiAdapter, Timing,
};

impl CardEffects for DispiAdapter {
    fn io(&self) -> Option<&Io> {
        self.io.as_ref()
    }

    fn display_configuration(&self) -> DisplayConfiguration {
        scanout::configuration(self)
    }

    fn timing(&self) -> Timing {
        match self.output.display.packed_layout() {
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
