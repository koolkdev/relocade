//! Card policy around synchronous standard VGA programming.
use super::Et4000Ax;
use crate::{
    effects::{Effects, VideoEffect},
    mmio::MemoryDecoding,
    ports::PortDecoding,
};
use relocade_machine::IoContext;

impl Et4000Ax {
    pub(super) fn with_effects<R>(
        &mut self,
        mut context: IoContext<'_>,
        effects: Effects,
        mutate: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if let Some(now) = self.output.prepare(effects.video) {
            self.advance_raster(now);
        }
        let previous_ports = effects.port_decode.then(|| PortDecoding::vga(&self.vga));
        let previous_memory = effects
            .memory_decode
            .then(|| MemoryDecoding::vga(&self.vga));
        let result = mutate(self);
        let video_changed = matches!(effects.video, VideoEffect::Configuration)
            && self
                .output
                .update_configuration(None, None, Self::vga_timing(&self.vga));
        let ports_changed = previous_ports.is_some_and(|previous| {
            let decoding = PortDecoding::vga(&self.vga);
            if previous == decoding {
                return false;
            }
            if let Some(io) = &self.io {
                io.ports
                    .update_decoding(context.reborrow(), Some(previous), decoding);
            }
            true
        });
        if let Some(previous) = previous_memory {
            let decoding = MemoryDecoding::vga(&self.vga);
            if previous != decoding {
                if let Some(io) = &self.io {
                    io.memory.update_decoding(context, Some(previous), decoding);
                }
            }
        }
        if video_changed || ports_changed {
            self.output.configuration_changed.notify(usize::MAX);
        }
        result
    }
}
