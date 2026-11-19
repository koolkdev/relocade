//! Limits retired instructions per host second.

use crate::{Clock, Sleep, TickRate, Timestamp};
use std::{
    future::Future,
    num::NonZeroU64,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};

pub(super) struct CpuLimit {
    rate: TickRate,
    burst: u64,
    start: Option<(Timestamp, Timestamp)>,
    wait: Option<Sleep>,
}

impl CpuLimit {
    pub(super) fn new(rate: NonZeroU64) -> Self {
        let rate = TickRate::new(rate);
        // Amortize host timer waits across four milliseconds of CPU work.
        let burst = rate
            .deadline_after(Timestamp::from_ticks(0), Duration::from_millis(4))
            .ticks();
        Self {
            rate,
            burst,
            start: None,
            wait: None,
        }
    }

    /// Returns the number of instructions left before the next throughput check.
    pub(super) fn poll(&mut self, host: &Clock, retired: u64, cx: &mut Context<'_>) -> Poll<u64> {
        let count = Timestamp::from_ticks(retired);
        let (first, began) = *self.start.get_or_insert_with(|| (count, host.now()));
        let used = retired.wrapping_sub(first.ticks());
        if used < self.burst {
            return Poll::Ready(self.burst - used);
        }

        // Charge actual retirement, including block overshoot. Wall time spent in
        // other work can cover this burst, but never earns an unlimited future burst.
        let end = host.deadline_after(began, self.rate.duration_between_ceil(first, count));
        if host.now() < end {
            let wait = self.wait.get_or_insert_with(|| host.sleep_until(end));
            ready!(Pin::new(wait).poll(cx));
        }
        self.wait = None;
        self.start = Some((count, host.now()));
        Poll::Ready(self.burst)
    }
}
