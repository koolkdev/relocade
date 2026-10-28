use std::{future::Future, time::Duration};
use web_time::Instant;

/// Shared elapsed session time and host timer opportunities.
/// Tests can supply manually advanced time. `wait_until` must yield to the host
/// even when the deadline is already due; a ready future is insufficient for a
/// browser worker to receive posted messages.
pub trait Clock {
    fn now(&self) -> Duration;
    fn wait_until(&self, deadline: Duration) -> impl Future<Output = ()>;
}

/// Wall time measured from this clock's construction on the execution worker.
/// Clones share the same elapsed-time origin.
#[derive(Clone)]
pub struct MonotonicClock {
    start: Instant,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    async fn wait_until(&self, deadline: Duration) {
        loop {
            let remaining = deadline.saturating_sub(self.now());
            futures_timer::Delay::new(remaining).await;
            if self.now() >= deadline {
                return;
            }
        }
    }
}
