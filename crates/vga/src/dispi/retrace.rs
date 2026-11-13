//! Card policy for waiting on vertical retrace.
use crate::{DispiAdapter, RetraceEvent};
use std::future::Future;

impl DispiAdapter {
    /// Prepare an owned wait without retaining a device borrow across suspension.
    /// Already being in retrace completes immediately. Suppressed sync waits
    /// for configuration changes to restore the signal; disabled VGA returns None.
    /// Notifications are installed before returning, so configuration changes
    /// invalidate even a wait that has not yet been polled. The caller handles
    /// revalidation after [`RetraceEvent::Reconfigured`]. Capture activity need
    /// not be running.
    pub fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        if !self.vga.setup.enabled() {
            return None;
        }
        self.advance_raster(self.output.clock.now());
        Some(self.output.wait_for_retrace())
    }
}
