//! One guest-time boundary for cooperative yields and optional host pacing.

use crate::{Clock, Schedule, Sleep, Timestamp};
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};

pub(super) struct ExecutionSlice {
    duration: Duration,
    end: Schedule,
    pub(super) paced: bool,
    wait: Option<Sleep>,
}

impl ExecutionSlice {
    pub(super) fn new(clock: &Clock, duration: Duration, paced: bool) -> Self {
        let mut end = clock.schedule(clock.now());
        end.advance(duration);
        Self {
            duration,
            end,
            paced,
            wait: None,
        }
    }

    pub(super) fn deadline(&self) -> Timestamp {
        self.end.deadline()
    }

    pub(super) fn poll(&mut self, clock: &Clock, now: Timestamp, cx: &mut Context<'_>) -> Poll<()> {
        let end = &mut self.end;
        if now < end.deadline() {
            return Poll::Ready(());
        }
        if self.paced {
            let sleep = self.wait.get_or_insert_with(|| clock.sync_to_host(now));
            ready!(Pin::new(sleep).poll(cx));
            self.wait = None;
        }
        // Advance from the retained boundary to preserve the slice grid even
        // when a CPU checkpoint overshoots by a fractional nanosecond.
        let elapsed = end.elapsed_until(now).as_nanos();
        let whole = elapsed / self.duration.as_nanos() * self.duration.as_nanos();
        end.advance(Duration::new(
            (whole / 1_000_000_000) as u64,
            (whole % 1_000_000_000) as u32,
        ));
        end.advance(self.duration);
        Poll::Ready(())
    }
}
