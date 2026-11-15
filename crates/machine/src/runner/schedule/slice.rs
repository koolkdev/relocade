//! One guest-time boundary for cooperative yields and optional host pacing.

use crate::{Clock, ClockCursor, Sleep, Timestamp};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};

pub(super) struct ExecutionSlice {
    duration: Duration,
    end: Cell<ClockCursor>,
    paced: bool,
    wait: RefCell<Option<Sleep>>,
}

impl ExecutionSlice {
    pub(super) fn new(clock: &Clock, duration: Duration, paced: bool) -> Self {
        let mut end = ClockCursor::from(clock.now());
        end.advance(clock, duration);
        Self {
            duration,
            end: Cell::new(end),
            paced,
            wait: RefCell::new(None),
        }
    }

    pub(super) fn deadline(&self) -> Timestamp {
        self.end.get().deadline()
    }

    /// Unpaced idle time can skip slices; a slice alone never advances time.
    pub(super) fn idle_deadline(&self, device_deadline: Timestamp) -> Timestamp {
        if self.paced {
            device_deadline.min(self.deadline())
        } else {
            device_deadline
        }
    }

    pub(super) fn cancel_wait(&self) {
        self.wait.borrow_mut().take();
    }

    pub(super) fn poll(&self, clock: &Clock, now: Timestamp, cx: &mut Context<'_>) -> Poll<()> {
        let mut end = self.end.get();
        if now < end.deadline() {
            return Poll::Ready(());
        }
        if self.paced {
            let mut wait = self.wait.borrow_mut();
            let sleep = wait.get_or_insert_with(|| clock.sync_to_host(now));
            ready!(Pin::new(sleep).poll(cx));
            wait.take();
        }
        // Advance from the retained boundary to preserve the slice grid even
        // when a CPU checkpoint overshoots by a fractional nanosecond.
        let elapsed = clock.duration_since(now, end).as_nanos();
        let whole = elapsed / self.duration.as_nanos() * self.duration.as_nanos();
        end.advance(
            clock,
            Duration::new(
                (whole / 1_000_000_000) as u64,
                (whole % 1_000_000_000) as u32,
            ),
        );
        end.advance(clock, self.duration);
        self.end.set(end);
        Poll::Ready(())
    }
}
