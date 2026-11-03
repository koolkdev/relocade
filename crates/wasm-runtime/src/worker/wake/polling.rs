//! One root polling adapter for a complete worker session.

use super::{WakeConnection, WakeTarget};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

impl<R: WakeTarget> WakeConnection<R> {
    /// Polls one complete session, including cancellation, with notifying wakers.
    /// Retain one instance of this adapter around the session's entire future;
    /// ordinary nested futures continue using their own combinator scheduling.
    /// A cancellation wrapper must register before entering busy execution.
    pub async fn run<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        let mut wakers: Option<(Waker, Waker)> = None;
        poll_fn(|cx| {
            if wakers
                .as_ref()
                .is_none_or(|(parent, _)| !parent.will_wake(cx.waker()))
            {
                wakers = Some((
                    cx.waker().clone(),
                    Waker::from(Arc::new(Notify {
                        notification: self.notification.clone(),
                        executor: cx.waker().clone(),
                        notified: self.notified.clone(),
                    })),
                ));
            }
            let (_, wrapped) = wakers.as_ref().unwrap();
            self.notified.store(false, Ordering::SeqCst);
            let result = future.as_mut().poll(&mut Context::from_waker(wrapped));
            // Keep the signal through the whole poll: CPU-first composition
            // must yield before its ready siblings can consume their input.
            self.acknowledge();
            // A wake after a sibling was polled still needs another checkpoint.
            // Record before requesting, and acknowledge before checking, so a
            // wake racing with acknowledgement cannot be lost.
            if self.notified.load(Ordering::SeqCst) {
                self.notification.wake_by_ref();
            }
            result
        })
        .await
    }

    /// Yields busy execution without requesting another interrupt. The current
    /// task's waker still runs, including any enclosing combinator's bookkeeping.
    pub async fn yield_now(&self) {
        let mut yielded = false;
        poll_fn(|cx| {
            if std::mem::replace(&mut yielded, true) {
                Poll::Ready(())
            } else {
                // Suppress only the first root notification from this child wake.
                // The executor's wake can itself publish unrelated ready work.
                let _scope = YieldScope(YIELDING.replace(Arc::as_ptr(&self.notified)));
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }
}

thread_local! {
    static YIELDING: Cell<*const AtomicBool> = const { Cell::new(std::ptr::null()) };
}

struct YieldScope(*const AtomicBool);
impl Drop for YieldScope {
    fn drop(&mut self) {
        YIELDING.set(self.0);
    }
}

struct Notify {
    notification: Waker,
    executor: Waker,
    notified: Arc<AtomicBool>,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if YIELDING.get() == Arc::as_ptr(&self.notified) {
            YIELDING.set(std::ptr::null());
        } else {
            self.notified.store(true, Ordering::SeqCst);
            self.notification.wake_by_ref();
        }
        self.executor.wake_by_ref();
    }
}
