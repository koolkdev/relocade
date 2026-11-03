//! Wakes backed by a resource installed on both ends of a worker connection.

use super::WorkerResource;
use std::{
    future::{poll_fn, Future},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

#[cfg_attr(not(target_arch = "wasm32"), path = "wake/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "wake/web.rs")]
mod platform;
pub use platform::{wake_connection, WakeConnection, WakeEndpoint, WakeHost};

/// A retained notification resource for one worker session.
/// Waking requests the running task's return to Rust; acknowledging clears that
/// request before polling its ready work. Native targets must also be `Send`.
/// Both operations must only update the signal, without invoking another waker.
pub trait WakeTarget: WorkerResource + Clone {
    fn wake(&self);
    fn acknowledge(&self);
}

async fn cooperate() {
    // An enclosing Abortable registers its waker only after Pending. Ensure
    // setup yields once before the session enters synchronous execution.
    let mut yielded = false;
    poll_fn(|cx| {
        if std::mem::replace(&mut yielded, true) {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

struct Notify {
    notification: Waker,
    executor: Waker,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notification.wake_by_ref();
        self.executor.wake_by_ref();
    }
}

async fn run<F: Future>(future: F, notification: &Waker, acknowledge: impl Fn()) -> F::Output {
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
                })),
            ));
        }
        let (_, wrapped) = wakers.as_ref().unwrap();
        // Clear first. A wake during this poll must remain set until another
        // poll drains the work, even when the executor coalesces notifications.
        acknowledge();
        future.as_mut().poll(&mut Context::from_waker(wrapped))
    })
    .await
}
