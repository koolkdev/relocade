//! ET4000 policy for the shared synchronous effects operation.
use super::Et4000Ax;
use crate::{
    effects::CardEffects, io::Io, mmio::MemoryDecoding, output::Output, ports::PortDecoding,
    PackedLayout, Timing,
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

    fn display_layout(&self) -> Option<PackedLayout> {
        self.packed_layout(self.display_start())
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
