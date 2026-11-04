//! Notifications from callback threads without a posted engine resource.

use super::LocalTarget;
use futures_util::task::AtomicWaker;
use std::{
    future::poll_fn,
    rc::Weak,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Poll, Wake},
};

/// Only Rust atomics cross threads. The host task alone accesses its JS handle.
pub(super) struct Relay {
    active: Arc<AtomicBool>,
    requested: AtomicBool,
    waiter: AtomicWaker,
}

impl Relay {
    pub(super) fn start(target: Weak<LocalTarget>, active: Arc<AtomicBool>) -> Arc<Self> {
        let relay = Arc::new(Self {
            active,
            requested: AtomicBool::new(false),
            waiter: AtomicWaker::new(),
        });
        let task = relay.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _registration = Registration(&task.waiter);
            poll_fn(|cx| {
                task.waiter.register(cx.waker());
                if !task.active.load(Ordering::SeqCst) {
                    return Poll::Ready(());
                }
                // Registration precedes the exchange: a request before registration
                // is retained, and a later request wakes this task for another poll.
                if task.requested.swap(false, Ordering::SeqCst) {
                    if let Some(target) = target.upgrade() {
                        (target.wake)();
                    }
                }
                Poll::Pending
            })
            .await;
        });
        relay
    }

    pub(super) fn request(self: &Arc<Self>) {
        if self.active.load(Ordering::SeqCst) && !self.requested.swap(true, Ordering::SeqCst) {
            self.wake_by_ref();
        }
    }
}

struct Registration<'a>(&'a AtomicWaker);
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.take();
    }
}

// Also used by both original connection owners after marking the connection
// inactive. Closing wakes the parked task to release its waiter on its next poll.
impl Wake for Relay {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.waiter.wake();
    }
}
