use futures_lite::future::poll_once;
use futures_timer::Delay;
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::{pin, Pin},
    task::ready,
    time::Duration,
};
use web_time::Instant;

/// A host turn must suspend the complete runnable group, including devices.
/// A timer in just the receive waiter cannot stop sibling Rust wakeups from
/// starving a browser MessagePort callback.
#[derive(Default)]
pub(super) struct HostEvents {
    waiters: Cell<usize>,
    since: Cell<Option<Instant>>,
}

impl HostEvents {
    pub(super) async fn wait<F: Future>(&self, result: F) -> F::Output {
        let mut result = pin!(result);
        if let Some(value) = poll_once(&mut result).await {
            return value;
        }
        if self.waiters.get() == 0 {
            self.since.set(Some(Instant::now()));
        }
        self.waiters.set(self.waiters.get() + 1);
        let _waiting = Waiting(self);
        result.await
    }

    pub(super) async fn run<F: Future>(&self, interval: Duration, future: F) -> F::Output {
        let mut future = pin!(future);
        let mut turn = None;
        poll_fn(|cx| {
            if turn.is_none()
                && self
                    .since
                    .get()
                    .is_some_and(|since| since.elapsed() >= interval)
            {
                turn = Some(Delay::new(Duration::ZERO));
            }
            if let Some(timer) = &mut turn {
                ready!(Pin::new(timer).poll(cx));
                turn = None;
                self.since.set((self.waiters.get() != 0).then(Instant::now));
            }
            future.as_mut().poll(cx)
        })
        .await
    }
}

struct Waiting<'a>(&'a HostEvents);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.waiters.set(self.0.waiters.get() - 1);
        if self.0.waiters.get() == 0 {
            self.0.since.set(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_channel::oneshot;
    use std::{
        future::{pending, ready},
        task::{Context, Poll, Waker},
    };

    #[test]
    fn completing_or_canceling_one_host_wait_preserves_the_others() {
        let host = HostEvents::default();
        let mut cx = Context::from_waker(Waker::noop());
        let mut immediate = pin!(host.wait(ready(42)));
        assert_eq!(immediate.as_mut().poll(&mut cx), Poll::Ready(42));
        assert!(host.since.get().is_none());

        let (send, received) = oneshot::channel();
        let mut completed = Box::pin(host.wait(received));
        let mut cancelled = Box::pin(host.wait(pending::<()>()));
        assert!(completed.as_mut().poll(&mut cx).is_pending());
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(host.waiters.get(), 2);
        drop(cancelled);
        assert_eq!(host.waiters.get(), 1);
        assert!(host.since.get().is_some());

        let mut remaining = Box::pin(host.wait(pending::<()>()));
        assert!(remaining.as_mut().poll(&mut cx).is_pending());
        send.send(()).unwrap();
        assert_eq!(completed.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
        assert_eq!(host.waiters.get(), 1);
        assert!(host.since.get().is_some());
        drop(remaining);
        assert_eq!(host.waiters.get(), 0);
        assert!(host.since.get().is_none());
    }
}
