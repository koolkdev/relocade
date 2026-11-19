//! Shared host-delivery timing for one executor.

use futures_timer::Delay;
use futures_util::future::LocalBoxFuture;
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::{pin, Pin},
    rc::Rc,
    task::{ready, Context, Poll, Waker},
    time::Duration,
};

pub(crate) struct HostSchedule {
    pub(crate) pending: HostTurns,
    timer: Option<Box<dyn Fn() -> LocalBoxFuture<'static, ()>>>,
}

impl HostSchedule {
    pub(super) fn new(wake: Waker) -> Self {
        Self {
            pending: HostTurns(Rc::new(Pending {
                count: Cell::new(0),
                wake,
            })),
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
    pub(super) fn idle(&mut self) {
        self.timer = None;
        self.turn = None;
    }

    /// Application work stays runnable during the grace interval and waits only
    /// for the host yield. Stop and parking remain with the executor.
    pub(super) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.schedule.pending.has_pending() {
            self.idle();
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

/// Registers work whose completion requires an event-loop turn on this worker.
/// Clones use the executor's shared grace timer while any such wait is pending.
#[derive(Clone)]
pub struct HostTurns(Rc<Pending>);

impl HostTurns {
    /// Polls the operation immediately. The first pending poll requests host
    /// delivery; completion or cancellation releases the request.
    pub async fn wait<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        let mut registration = None;
        poll_fn(|cx| {
            let result = future.as_mut().poll(cx);
            if result.is_pending() && registration.is_none() {
                let pending = &*self.0;
                let count = pending.count.get();
                pending.count.set(count + 1);
                registration = Some(Registration(pending));
                if count == 0 {
                    pending.wake.wake_by_ref();
                }
            }
            result
        })
        .await
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.0.count.get() != 0
    }
}

struct Pending {
    count: Cell<usize>,
    wake: Waker,
}

struct Registration<'a>(&'a Pending);

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let remaining = self.0.count.get() - 1;
        self.0.count.set(remaining);
        if remaining == 0 {
            self.0.wake.wake_by_ref();
        }
    }
}
