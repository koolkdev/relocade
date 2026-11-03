//! Wakes backed by a resource installed on both ends of a worker connection.

use super::WorkerResource;
use crate::Result;
use std::{
    cell::{Cell, RefCell},
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
pub use platform::{wake_connection, WakeEndpoint, WakeHost};

/// A retained notification resource for one worker session.
/// Waking requests the running task's return to Rust; acknowledging clears that
/// request after polling its ready work. Clones and posted reconstructions must
/// address the same signal. Native targets must also be `Send`.
/// Waking and acknowledgement may run concurrently on different clones.
/// Both operations must only update the signal, without invoking another waker.
pub trait WakeTarget: WorkerResource + Clone {
    fn wake(&self);
    fn acknowledge(&self);
}

/// The execution end of a session's wake connection.
/// Owns activation, polling and acknowledgement on the execution thread.
pub struct WakeConnection<R: WakeTarget> {
    binding: platform::Binding<R>,
    target: RefCell<Option<R>>,
    notification: Waker,
    notified: Arc<AtomicBool>,
}

impl<R: WakeTarget> WakeConnection<R> {
    fn new(binding: platform::Binding<R>, notification: Waker) -> Self {
        Self {
            binding,
            target: RefCell::new(None),
            notification,
            notified: Arc::default(),
        }
    }

    /// Installs the target before busy execution. Browser installation waits for
    /// host acknowledgement; native access is available immediately. Replacing
    /// a CPU replaces the notification target for this session.
    pub async fn activate(&self, target: R) -> Result<()> {
        let previous = self.target.replace(Some(target.clone()));
        drop(previous);
        self.binding.install(target).await?;
        // Abortable registers only after Pending, before busy execution starts.
        self.yield_now().await;
        Ok(())
    }

    /// Polls the entire session, including cancellation, with notifying wakers.
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
            if let Some(target) = &*self.target.borrow() {
                target.acknowledge();
            }
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
                // The scope covers only this connection's synchronous wake chain
                // on this thread. Other notifications must still interrupt.
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
        if YIELDING.get() != Arc::as_ptr(&self.notified) {
            self.notified.store(true, Ordering::SeqCst);
            self.notification.wake_by_ref();
        }
        self.executor.wake_by_ref();
    }
}
