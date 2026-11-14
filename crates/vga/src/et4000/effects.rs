//! ET4000 policy for the shared synchronous effects operation.
use super::{scanout, Et4000Ax};
use crate::{
    effects::CardEffects, io::Io, mmio::MemoryDecoding, ports::PortDecoding,
    scanout::DisplayConfiguration, Timing,
};

impl CardEffects for Et4000Ax {
    fn io(&self) -> Option<&Io> {
        self.io.as_ref()
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
