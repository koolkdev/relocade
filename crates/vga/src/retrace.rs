//! Owned waits prepared from the live device's vertical retrace timing.

use crate::Vga;
use futures_lite::future::or;
use relocade_machine::Clock;
use std::{future::Future, time::Duration};

/// What completed a wait prepared by [`Vga::wait_for_retrace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetraceEvent {
    /// Retrace occurred; its pulse may have ended before the caller resumes.
    Started,
    /// Device programming changed; revalidate the request before waiting again.
    Reprogrammed,
}

impl<C: Clock + 'static> Vga<C> {
    /// Prepare an owned wait without retaining a device borrow across suspension.
    /// Already being in retrace completes immediately. Suppressed sync waits
    /// for programming to restore the signal; disabled VGA returns None.
    /// Notifications are installed before returning, so intervening programming
    /// invalidates even a wait that has not yet been polled. The caller handles
    /// revalidation after [`RetraceEvent::Reprogrammed`]. Capture activity need
    /// not be running.
    pub fn wait_for_retrace(&mut self) -> Option<impl Future<Output = RetraceEvent> + 'static> {
        if !self.setup.enabled() {
            return None;
        }
        self.sync_to_clock();
        let deadline = if self.raster.in_retrace() {
            Some(self.clock.now())
        } else {
            self.raster.next_retrace_pulse_at()
        };
        Some(self.retrace_wait(deadline))
    }

    /// Install both wake paths before releasing the device borrow. Programming
    /// takes priority when the timer and notification are both ready.
    pub(super) fn retrace_wait(
        &self,
        deadline: Option<Duration>,
    ) -> impl Future<Output = RetraceEvent> + 'static {
        let clock = self.clock.clone();
        let changed = self.programming_changed.listen();
        async move {
            let changed = async {
                changed.await;
                RetraceEvent::Reprogrammed
            };
            if let Some(deadline) = deadline {
                or(changed, async {
                    if clock.now() < deadline {
                        clock.wait_until(deadline).await;
                    }
                    RetraceEvent::Started
                })
                .await
            } else {
                changed.await
            }
        }
    }
}
