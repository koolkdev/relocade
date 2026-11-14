//! DISPI's retrace-latch policy and public output access.

use crate::{
    output::{CardOutput, Output},
    DispiAdapter, DisplaySnapshot, Framebuffer, FramebufferAccess, RetraceEvent,
};
use relocade_machine::Timestamp;
use std::future::Future;

impl CardOutput for DispiAdapter {
    fn output(&mut self) -> &mut Output {
        &mut self.output
    }

    fn adapter_enabled(&self) -> bool {
        self.vga.setup.enabled()
    }

    fn capture_retrace(&mut self, time: Timestamp) {
        if self.output.display.packed_layout().is_none() {
            self.vga.registers.crtc.latch_display_start();
        }
        self.output.capture(
            &self.vga.registers,
            &mut self.memory,
            u32::from(self.vga.registers.crtc.display_start),
            time,
        );
    }
}

impl DispiAdapter {
    /// Captures current display inputs immediately on the execution owner, without
    /// advancing time, publishing a frame, or changing CPU read/access latches.
    /// Copies written memory blocks and the active display settings and DAC.
    /// Leaves the output baseline and pending dirty pages unchanged.
    /// Packed SVGA captures own their layout independently of the CPU bank.
    /// The display-start latch stays at the last serviced VGA retrace; this does not
    /// force pending CRTC start-address writes to become effective.
    /// Unlike [`DispiAdapter::frames`], this does not wait for a refresh boundary.
    pub fn snapshot(&self) -> DisplaySnapshot {
        self.output.snapshot(
            &self.vga.registers,
            &self.memory,
            u32::from(self.vga.registers.crtc.display_start),
        )
    }

    /// Prepare an owned wait without retaining a device borrow across suspension.
    /// Already being in retrace completes immediately. Suppressed sync waits
    /// for configuration changes to restore the signal; disabled VGA returns None.
    /// Notifications are installed before returning, so configuration changes
    /// invalidate even a wait that has not yet been polled. The caller handles
    /// revalidation after [`RetraceEvent::Reconfigured`]. Capture activity need
    /// not be running.
    pub fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        CardOutput::wait_for_retrace(self)
    }
}

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let start = u32::from(self.vga.registers.crtc.programmed_start());
        self.output
            .framebuffer(&self.vga.registers, &mut self.memory, start)
    }
}
