//! Event-loop waits on the timer worker; cross-thread wakes contain only Rust state.

use futures_util::task::AtomicWaker;
use std::{
    future::{poll_fn, Future},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Poll, Wake, Waker},
};
use web_time::Instant;

pub(super) mod environment;
mod timer;

pub(super) struct Wait {
    signal: Arc<Signal>,
    timer: timer::Timer,
}

impl Wait {
    pub fn new() -> Self {
        Self {
            signal: Arc::new(Signal {
                notified: AtomicBool::new(false),
                waiter: AtomicWaker::new(),
            }),
            timer: timer::Timer::new(),
        }
    }

    pub fn waker(&self) -> Waker {
        Waker::from(self.signal.clone())
    }

    pub async fn until(&mut self, deadline: Option<Instant>) {
        let _registration = Registration(&self.signal.waiter);
        let mut timeout = deadline.map(|deadline| self.timer.start(deadline));
        poll_fn(|cx| {
            // Register before consuming the retained token so a concurrent
            // request cannot fall between the readiness check and registration.
            self.signal.waiter.register(cx.waker());
            if self.signal.notified.swap(false, Ordering::AcqRel) {
                return Poll::Ready(());
            }
            if let Some(timeout) = &mut timeout {
                if Pin::new(timeout).poll(cx).is_ready() {
                    return Poll::Ready(());
                }
            }
            Poll::Pending
        })
        .await;
    }
}

struct Signal {
    notified: AtomicBool,
    waiter: AtomicWaker,
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notified.store(true, Ordering::Release);
        self.waiter.wake();
    }
}

struct Registration<'a>(&'a AtomicWaker);
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.take();
    }
}

#[cfg(test)]
mod tests;
