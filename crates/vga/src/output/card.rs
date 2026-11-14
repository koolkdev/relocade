//! Shared output flow with each card's retrace-latch policy.
use super::{Output, RetraceEvent};
use relocade_machine::Timestamp;
use std::future::Future;

/// Concrete cards supply their output state and apply hardware latches at retrace.
/// Calls are specialized for each card, including the cached status slow path.
pub(crate) trait CardOutput: Sized {
    fn output(&mut self) -> &mut Output;
    fn adapter_enabled(&self) -> bool;
    fn capture_retrace(&mut self, time: Timestamp);

    /// Service due captures from the output task or before display programming.
    /// CPU VRAM transfers do not consult the clock.
    fn capture_if_due(&mut self) {
        if let Some(now) = self.output().capture_due() {
            self.advance_raster(now);
        }
    }

    /// Timing changes materialize any remaining phase after servicing due output.
    #[inline]
    fn advance_raster(&mut self, now: Timestamp) {
        if self.output().raster.status.observe(now) {
            self.refresh_raster();
        }
    }

    // Status polling and the output task enter the same retrace/capture path.
    #[cold]
    #[inline(never)]
    fn refresh_raster(&mut self) {
        if let Some(time) = self.output().raster.update_observed() {
            self.capture_retrace(time);
        }
    }

    /// Disabled adapter decoding has no retrace wait. Install notifications
    /// before returning an owned future, without retaining a device borrow.
    fn wait_for_retrace(
        &mut self,
    ) -> Option<impl Future<Output = RetraceEvent> + 'static + use<Self>> {
        if !self.adapter_enabled() {
            return None;
        }
        let now = self.output().clock.now();
        self.advance_raster(now);
        Some(self.output().wait_for_retrace())
    }
}
