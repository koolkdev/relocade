//! ET4000 policy for the shared synchronous effects operation.
use super::Et4000Ax;
use crate::{
    effects::{CardEffects, VideoEffect},
    io::Io,
    mmio::MemoryDecoding,
    output::Output,
    ports::PortDecoding,
    PackedLayout, Scanout, Timing,
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
        match self.output.scanout {
            Scanout::Packed(layout) => Some(layout),
            Scanout::Vga(_) => None,
        }
    }

    fn update_scanout(&mut self, effect: VideoEffect) {
        if matches!(effect, VideoEffect::None) {
            // CPU-only controls remain visible in raw VGA snapshots without
            // rebuilding the active display's layout or blanking intervals.
            if matches!(self.output.scanout, Scanout::Vga(_)) {
                self.output.scanout = Scanout::Vga(self.vga_display_state());
            }
            return;
        }
        // Prepare a relative packed viewport. Retrace supplies its latched
        // display-start counter; native drawing supplies the programmed one.
        self.output.scanout = self
            .packed_layout()
            .map(Scanout::Packed)
            .unwrap_or_else(|| Scanout::Vga(self.vga_display_state()));
        self.output.blanked = !self.vga.registers.attribute.display_enabled()
            || self.vga.registers.sequencer.reset_active()
            || self.vga.registers.sequencer.values[1] & 0x20 != 0;
        self.output.blanking = if matches!(self.output.scanout, Scanout::Packed(_)) {
            self.packed_blanking()
        } else {
            Default::default()
        };
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
