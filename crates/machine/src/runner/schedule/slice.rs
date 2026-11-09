//! One guest-time boundary for cooperative yields and optional host pacing.

use crate::{GuestTimeDriver, Instant, Sleep};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};

pub(super) struct ExecutionSlice {
    duration: Duration,
    end: Cell<Instant>,
    paced: bool,
    wait: RefCell<Option<Sleep>>,
}

impl ExecutionSlice {
    pub(super) fn new(start: Instant, duration: Duration, paced: bool) -> Self {
        Self {
            duration,
            end: Cell::new(start + duration),
            paced,
            wait: RefCell::new(None),
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        self.end.get()
    }

    /// Unpaced idle time can skip slices; a slice alone never advances time.
    pub(super) fn idle_deadline(&self, device_deadline: Instant) -> Instant {
        if self.paced {
            device_deadline.min(self.end.get())
        } else {
            device_deadline
        }
    }

    pub(super) fn cancel_wait(&self) {
        self.wait.borrow_mut().take();
    }

    pub(super) fn poll(
        &self,
        driver: &GuestTimeDriver,
        now: Instant,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        let end = self.end.get();
        if now < end {
            return Poll::Ready(());
        }
        if self.paced {
            let mut wait = self.wait.borrow_mut();
            let sleep = wait.get_or_insert_with(|| driver.host_sleep_until(now));
            ready!(Pin::new(sleep).poll(cx));
            wait.take();
        }
        // Carry checkpoint overshoot forward instead of shifting the slice grid.
        let remaining = self.duration.as_nanos()
            - now.duration_since(end).as_nanos() % self.duration.as_nanos();
        self.end.set(
            now + Duration::new(
                (remaining / 1_000_000_000) as u64,
                (remaining % 1_000_000_000) as u32,
            ),
        );
        Poll::Ready(())
    }
}
