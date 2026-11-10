//! Shared host-delivery timing for one executor.

use futures_util::future::LocalBoxFuture;
use std::{
    cell::Cell,
    future::Future,
    rc::Rc,
    task::{Context, Waker},
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
            turn_requested: false,
        }
    }
}

pub(super) struct HostRun<'a> {
    schedule: &'a HostSchedule,
    timer: Option<LocalBoxFuture<'static, ()>>,
    turn_requested: bool,
}

impl HostRun<'_> {
    /// Arm before polling work: synchronous guest execution can run until the
    /// timer wakes this executor and requests an execution checkpoint.
    pub(super) fn poll(&mut self, cx: &mut Context<'_>) {
        if !self.schedule.pending.has_pending() {
            self.timer = None;
            return;
        }
        self.turn_requested = match &self.schedule.timer {
            Some(timer) => self
                .timer
                .get_or_insert_with(timer)
                .as_mut()
                .poll(cx)
                .is_ready(),
            None => true,
        };
        if self.turn_requested {
            self.schedule.pending.wake.wake_by_ref();
        }
    }

    /// Recheck after polling work: delivery or cancellation may have ended the wait.
    pub(super) fn take_turn(&mut self) -> bool {
        let pending = self.schedule.pending.has_pending();
        let requested = std::mem::take(&mut self.turn_requested);
        if !pending || requested {
            self.timer = None;
        }
        pending && requested
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
