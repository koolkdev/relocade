//! ET4000's retrace-latch policy and public output access.
use super::Et4000Ax;
use crate::{
    output::{CardOutput, Output},
    DisplaySnapshot, Framebuffer, FramebufferAccess, RetraceEvent,
};
use relocade_machine::Timestamp;
use std::future::Future;

impl CardOutput for Et4000Ax {
    fn output(&mut self) -> &mut Output {
        &mut self.output
    }

    fn adapter_enabled(&self) -> bool {
        self.vga.setup.enabled()
    }

    fn capture_retrace(&mut self, time: Timestamp) {
        self.vga.registers.crtc.latch_display_start();
        self.display_start_high = self
            .extensions
            .crtc(super::registers::EXTENDED_START)
            .unwrap()
            & 3;
        let start = self.display_start();
        self.output
            .capture(&self.vga.registers, &mut self.memory, start, time);
    }
}

impl Et4000Ax {
    /// Inspect current display inputs without advancing the raster or CPU latches.
    pub fn snapshot(&self) -> DisplaySnapshot {
        self.output
            .snapshot(&self.vga.registers, &self.memory, self.display_start())
    }

    /// Prepare an owned wait; disabled decoding returns None.
    pub fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        CardOutput::wait_for_retrace(self)
    }
}

impl FramebufferAccess for Et4000Ax {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let start = self.programmed_start();
        self.output
            .framebuffer(&self.vga.registers, &mut self.memory, start)
    }
}
