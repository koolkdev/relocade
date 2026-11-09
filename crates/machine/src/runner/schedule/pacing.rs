//! A retained execution allowance, shared by busy and idle guest time.

use crate::{GuestTimeDriver, Instant, Sleep};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};

pub(super) struct Pacing {
    interval: Duration,
    end: Cell<Option<Instant>>,
    wait: RefCell<Option<Sleep>>,
}

impl Pacing {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            end: Cell::new(None),
            wait: RefCell::new(None),
        }
    }

    pub(super) fn begin(&self, now: Instant) {
        if self.end.get().is_none() {
            self.end.set(Some(now + self.interval));
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        self.end.get().expect("pacing belongs to an active run")
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
        let end = self.deadline();
        if now < end {
            return Poll::Ready(());
        }
        let mut wait = self.wait.borrow_mut();
        let sleep = wait.get_or_insert_with(|| driver.host_sleep_until(now));
        ready!(Pin::new(sleep).poll(cx));
        wait.take();
        // Carry checkpoint overshoot forward instead of moving the burst grid.
        self.end.set(Some(next_boundary(now, end, self.interval)));
        Poll::Ready(())
    }
}

pub(super) fn next_boundary(now: Instant, origin: Instant, interval: Duration) -> Instant {
    let remaining =
        interval.as_nanos() - now.duration_since(origin).as_nanos() % interval.as_nanos();
    now + Duration::new(
        (remaining / 1_000_000_000) as u64,
        (remaining % 1_000_000_000) as u32,
    )
}
