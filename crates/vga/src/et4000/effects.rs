//! ET4000 policy for the shared synchronous effects operation.
use super::{scanout, Et4000Ax};
use crate::{
    effects::CardEffects, io::Io, mmio::MemoryDecoding, output::Output, ports::PortDecoding,
    scanout::DisplayConfiguration, Timing,
};
use relocade_machine::Timestamp;

impl CardEffects for Et4000Ax {
    fn output(&mut self) -> &mut Output {
        &mut self.output
    }

    fn io(&self) -> Option<&Io> {
        self.io.as_ref()
    }

    fn advance_raster(&mut self, now: Timestamp) {
        Et4000Ax::advance_raster(self, now);
    }

    fn display_configuration(&self) -> DisplayConfiguration {
        scanout::configuration(self)
    }

    fn timing(&self) -> Timing {
        Self::display_timing(&self.vga, &self.extensions)
    }

    fn port_decoding(&self) -> PortDecoding {
        PortDecoding::vga(&self.vga)
    }

    fn memory_decoding(&self) -> MemoryDecoding {
        Et4000Ax::memory_decoding(self)
    }
}
