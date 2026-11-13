//! Standard VGA display interpretation and retrace-latch policy for this card.
use super::Et4000Ax;
use crate::{DacSnapshot, DisplaySnapshot, Framebuffer, FramebufferAccess, RetraceEvent, Scanout};
use relocade_machine::Timestamp;
use std::future::Future;

impl Et4000Ax {
    /// Inspect current display inputs without advancing the raster or CPU latches.
    pub fn snapshot(&self) -> DisplaySnapshot {
        self.output.snapshot(
            Scanout::Vga(self.vga_display_state()),
            DacSnapshot {
                colors: self.vga.registers.dac.colors,
                mask: self.vga.registers.dac.mask,
            },
            &self.memory,
        )
    }

    pub(super) fn capture_if_due(&mut self) {
        if let Some(now) = self.output.capture_due() {
            self.advance_raster(now);
        }
    }

    pub(super) fn advance_raster(&mut self, now: Timestamp) {
        if self.output.raster.status.observe(now) {
            self.refresh_raster();
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn refresh_raster(&mut self) {
        let Some(time) = self.output.raster.update_observed() else {
            return;
        };
        self.vga.registers.crtc.latch_display_start();
        let snapshot = self.snapshot();
        self.output.capture(snapshot, &mut self.memory, time);
    }

    /// Prepare an owned wait; disabled decoding returns None.
    pub fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        if !self.vga.setup.enabled() {
            return None;
        }
        self.advance_raster(self.output.clock.now());
        Some(self.output.wait_for_retrace())
    }
}

impl FramebufferAccess for Et4000Ax {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let mut state = self.vga_display_state();
        let crtc = &self.vga.registers.crtc.registers.values;
        state.display_start = u16::from_be_bytes([crtc[0x0c], crtc[0x0d]]);
        Framebuffer {
            scanout: Scanout::Vga(state),
            memory: &mut self.memory,
        }
    }
}
