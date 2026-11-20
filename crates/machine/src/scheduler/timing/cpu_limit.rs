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
    host: Clock,
    rate: TickRate,
    burst: u64,
    start: Option<(Timestamp, Timestamp)>,
    wait: Option<Sleep>,
}

impl CpuLimit {
    pub(super) fn new(rate: NonZeroU64, host: Clock) -> Self {
        let rate = TickRate::new(rate);
        // Amortize host timer waits across four milliseconds of CPU work.
        let burst = rate
            .deadline_after(Timestamp::from_ticks(0), Duration::from_millis(4))
            .ticks();
        Self {
            host,
            rate,
            burst,
            start: None,
            wait: None,
        }
    }

    pub(super) fn remaining(&self, retired: u64) -> u64 {
        self.start.map_or(self.burst, |(first, _)| {
            self.burst
                .saturating_sub(retired.wrapping_sub(first.ticks()))
        })
    }

    /// Returns the number of instructions left before the next throughput check.
    pub(super) fn allowance(&mut self, retired: u64, cx: &mut Context<'_>) -> Poll<u64> {
        let host = &self.host;
        let count = Timestamp::from_ticks(retired);
        let (first, began) = *self.start.get_or_insert_with(|| (count, host.now()));
        let remaining = self.remaining(retired);
        if remaining != 0 {
            return Poll::Ready(remaining);
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
