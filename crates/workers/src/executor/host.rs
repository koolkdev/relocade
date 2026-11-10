//! Shared host-delivery timing for one executor.

use futures_timer::Delay;
use futures_util::future::LocalBoxFuture;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{ready, Context, Poll, Waker},
    time::Duration,
};

pub(crate) struct HostSchedule {
    pub(crate) pending: Rc<PendingDeliveries>,
    timer: Option<Box<dyn Fn() -> LocalBoxFuture<'static, ()>>>,
}

impl HostSchedule {
    pub(super) fn new(wake: Waker) -> Self {
        Self {
            pending: Rc::new(PendingDeliveries {
                count: Cell::new(0),
                wake,
            }),
            timer: None,
        }
    }

    pub(super) fn set_timer<F: Future<Output = ()> + 'static>(
        &mut self,
        interval: Duration,
        timer: impl Fn(Duration) -> F + 'static,
    ) {
        self.timer = Some(Box::new(move || Box::pin(timer(interval))));
    }

    /// Keep the active timer local to the run so stopping drops it immediately.
    pub(super) fn run(&self) -> HostRun<'_> {
        HostRun {
            schedule: self,
            timer: None,
            turn: None,
        }
    }
}

pub(super) struct HostRun<'a> {
    schedule: &'a HostSchedule,
    timer: Option<LocalBoxFuture<'static, ()>>,
    turn: Option<Delay>,
}

impl HostRun<'_> {
    /// Application work stays runnable during the grace interval and waits only
    /// for the host yield. Stop and parking remain with the executor.
    pub(super) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.schedule.pending.has_pending() {
            self.timer = None;
            self.turn = None;
            return Poll::Ready(());
        }

        if let Some(turn) = &mut self.turn {
            ready!(Pin::new(turn).poll(cx));
            self.turn = None;
            // Arm the next interval before synchronous work resumes. Without
            // a grace interval, let work run once and request its next checkpoint.
            if self.interval_elapsed(cx) {
                self.turn = Some(Delay::new(Duration::ZERO));
                cx.waker().wake_by_ref();
            }
            return Poll::Ready(());
        }

        if !self.interval_elapsed(cx) {
            return Poll::Ready(());
        }
        self.turn = Some(Delay::new(Duration::ZERO));
        // Poll the new wait through the executor's ordinary readiness loop.
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn interval_elapsed(&mut self, cx: &mut Context<'_>) -> bool {
        let elapsed = match &self.schedule.timer {
            Some(timer) => self
                .timer
                .get_or_insert_with(timer)
                .as_mut()
                .poll(cx)
                .is_ready(),
            None => true,
        };
        if elapsed {
            self.timer = None;
        }
        elapsed
    }
}

pub(crate) struct PendingDeliveries {
    count: Cell<usize>,
    wake: Waker,
}

impl PendingDeliveries {
    pub(crate) fn register(&self) -> Delivery<'_> {
        let count = self.count.get();
        self.count.set(count + 1);
        if count == 0 {
            // A later sibling may enter guest code during this same root poll.
            // Interrupt it so the executor can arm the shared delivery timer.
            self.wake.wake_by_ref();
        }
        Delivery(self)
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.count.get() != 0
    }
}

pub(crate) struct Delivery<'a>(&'a PendingDeliveries);

impl Drop for Delivery<'_> {
    fn drop(&mut self) {
        self.0.count.set(self.0.count.get() - 1);
        if !self.0.has_pending() {
            // Let the executor cancel its timer when the last receive ends.
            self.0.wake.wake_by_ref();
        }
    }
}
