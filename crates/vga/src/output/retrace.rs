//! Owned waits tied to one card's display configuration.
use super::Output;
use futures_lite::future::or;
use relocade_machine::Timestamp;
use std::future::Future;

/// What completed a wait for the card's vertical retrace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetraceEvent {
    /// Retrace occurred; its pulse may have ended before the caller resumes.
    Started,
    /// Video configuration or port decoding changed; prepare a new wait.
    Reconfigured,
}

impl Output {
    pub fn wait_for_retrace(&self) -> impl Future<Output = RetraceEvent> + 'static {
        let deadline = if self.raster.in_retrace() {
            Some(self.clock.now())
        } else {
            self.raster.next_retrace_pulse_at()
        };
        self.retrace_wait(deadline)
    }

    /// Install both wake paths before releasing the device borrow. Configuration
    /// changes take priority when the timer and notification are both ready.
    pub(crate) fn retrace_wait(
        &self,
        deadline: Option<Timestamp>,
    ) -> impl Future<Output = RetraceEvent> + 'static {
        let clock = self.clock.clone();
        let changed = self.configuration_changed.listen();
        async move {
            let changed = async {
                changed.await;
                RetraceEvent::Reconfigured
            };
            if let Some(deadline) = deadline {
                or(changed, async {
                    if clock.now() < deadline {
                        clock.sleep_until(deadline).await;
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
