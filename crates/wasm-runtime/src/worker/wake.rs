//! Wakes backed by a resource installed on both ends of a worker connection.

use super::WorkerResource;
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

#[cfg_attr(not(target_arch = "wasm32"), path = "wake/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "wake/web.rs")]
mod platform;
pub use platform::{wake_connection, WakeConnection, WakeEndpoint, WakeHost};

/// A retained notification resource for one worker session.
/// Waking requests the running task's return to Rust; acknowledging clears that
/// request after polling its ready work. Native targets must also be `Send`.
/// Both operations must only update the signal, without invoking another waker.
pub trait WakeTarget: WorkerResource + Clone {
    fn wake(&self);
    fn acknowledge(&self);
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
    polling: Arc<AtomicBool>,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if YIELDING.get() != Arc::as_ptr(&self.polling) {
            self.polling.store(true, Ordering::SeqCst);
            self.notification.wake_by_ref();
        }
        self.executor.wake_by_ref();
    }
}

#[derive(Default)]
struct Polling(Arc<AtomicBool>);

impl Polling {
    async fn yield_now(&self) {
        let mut yielded = false;
        poll_fn(|cx| {
            if std::mem::replace(&mut yielded, true) {
                Poll::Ready(())
            } else {
                // Wake the current child, including combinator bookkeeping,
                // without treating our own cooperative yield as new input.
                // The scope is local to this connection and this thread.
                let _scope = YieldScope(YIELDING.replace(Arc::as_ptr(&self.0)));
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }

    async fn run<F: Future>(
        &self,
        future: F,
        notification: &Waker,
        acknowledge: impl Fn(),
    ) -> F::Output {
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
                        notification: notification.clone(),
                        executor: cx.waker().clone(),
                        polling: self.0.clone(),
                    })),
                ));
            }
            let (_, wrapped) = wakers.as_ref().unwrap();
            self.0.store(false, Ordering::SeqCst);
            let result = future.as_mut().poll(&mut Context::from_waker(wrapped));
            // Keep the signal through the whole poll: CPU-first composition
            // must yield before its ready siblings can consume their input.
            acknowledge();
            // A wake after a sibling was polled still needs another checkpoint.
            // Record before requesting, and acknowledge before checking, so a
            // wake racing with acknowledgement cannot be lost.
            if self.0.load(Ordering::SeqCst) {
                notification.wake_by_ref();
            }
            result
        })
        .await
    }
}
