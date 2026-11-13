//! Capture on the execution owner, then synchronize and publish from its task.

use crate::{DacSnapshot, DispiAdapter, DisplaySnapshot, Scanout};
use relocade_machine::Timestamp;

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
            self.scanout_at(self.vga.registers.crtc.display_start),
            DacSnapshot {
                colors: self.vga.registers.dac.colors,
                mask: self.vga.registers.dac.mask,
            },
            &self.memory,
        )
    }

    pub(super) fn scanout_at(&self, start: u16) -> Scanout {
        match self.output.scanout {
            Scanout::Vga(mut state) => {
                state.display_start = start;
                Scanout::Vga(state.with_blink_phase(self.output.raster.retrace_count))
            }
            packed => packed,
        }
    }

    /// Service elapsed capture boundaries from the output task or before a
    /// display programming change. CPU VRAM transfers do not consult the clock.
    pub(super) fn capture_if_due(&mut self) {
        if let Some(now) = self.output.capture_due() {
            self.advance_raster(now);
        }
    }

    /// Refresh due status and capture events. Timing changes then materialize
    /// any remaining phase at this observed time before remapping the counters.
    #[inline]
    pub(super) fn advance_raster(&mut self, now: Timestamp) {
        if self.output.raster.status.observe(now) {
            self.refresh_raster();
        }
    }

    // Status readers and the output task share raster integration and capture.
    // Mode selection is only needed when a retrace produces new display inputs.
    #[cold]
    #[inline(never)]
    pub(super) fn refresh_raster(&mut self) {
        let Some(time) = self.output.raster.update_observed() else {
            return;
        };
        if self.dispi.display_layout().is_none() {
            self.vga.registers.crtc.latch_display_start();
        }
        let display = self.snapshot();
        self.output.capture(display, &mut self.memory, time);
    }
}
