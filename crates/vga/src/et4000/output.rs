//! Captured display inputs and retrace-latch policy for this card.
use super::Et4000Ax;
use crate::{DacSnapshot, DisplaySnapshot, Framebuffer, FramebufferAccess, RetraceEvent};
use relocade_machine::Timestamp;
use std::future::Future;

impl Et4000Ax {
    /// Inspect current display inputs without advancing the raster or CPU latches.
    pub fn snapshot(&self) -> DisplaySnapshot {
        self.output.snapshot(
            self.scanout_at(self.display_start()),
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
        self.display_start_high = self
            .extensions
            .crtc(super::registers::EXTENDED_START)
            .unwrap()
            & 3;
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
        Framebuffer {
            scanout: self.scanout_at(self.programmed_start()),
            memory: &mut self.memory,
        }
    }
}
