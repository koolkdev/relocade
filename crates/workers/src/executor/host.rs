//! Shared host-delivery timing for one executor.

use futures_util::future::LocalBoxFuture;
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};
use web_time::Instant;

pub(crate) struct HostSchedule {
    pub(crate) pending: HostTurns,
    start_grace: Option<Box<dyn Fn() -> Grace>>,
}

pub(super) enum Grace {
    Timer(LocalBoxFuture<'static, ()>),
    Elapsed { start: Instant, interval: Duration },
}

impl HostSchedule {
    pub(super) fn new(interrupt: Waker) -> Self {
        Self {
            pending: HostTurns(Rc::new(Pending {
                count: Cell::new(0),
                interrupt,
            })),
            start_grace: None,
        }
    }

    pub(super) fn set_timer<F: Future<Output = ()> + 'static>(
        &mut self,
        interval: Duration,
        timer: impl Fn(Duration) -> F + 'static,
    ) {
        self.start_grace = Some(Box::new(move || Grace::Timer(Box::pin(timer(interval)))));
    }

    pub(super) fn cooperative_grace(&mut self, interval: Duration) {
        self.start_grace = Some(Box::new(move || Grace::Elapsed {
            start: Instant::now(),
            interval,
        }));
    }

    /// While work can run, give a pending delivery one shared grace interval.
    pub(super) fn turn_due(&self, grace: &mut Option<Grace>, cx: &mut Context<'_>) -> bool {
        if !self.pending.has_pending() {
            *grace = None;
            return false;
        }
        let Some(start_grace) = &self.start_grace else {
            return true;
        };
        let due = match grace.get_or_insert_with(start_grace) {
            Grace::Timer(timer) => timer.as_mut().poll(cx).is_ready(),
            Grace::Elapsed { start, interval } => {
                // Sample the clock again after the next busy CPU checkpoint.
                self.pending.0.interrupt.wake_by_ref();
                start.elapsed() >= *interval
            }
        };
        if due {
            *grace = None;
        }
        due
    }
}

/// Registers work whose completion requires an event-loop turn on this worker.
/// Clones share one grace interval while any such wait is pending.
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
                    pending.interrupt.wake_by_ref();
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
    interrupt: Waker,
}

struct Registration<'a>(&'a Pending);

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.count.set(self.0.count.get() - 1);
    }
}
