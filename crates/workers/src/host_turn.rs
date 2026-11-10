//! Host event-loop opportunities requested by posted-message receivers.

use futures_channel::oneshot;
use futures_util::future::{select, Either, LocalBoxFuture};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    task::{Poll, Waker},
    time::Duration,
};

pub(crate) struct HostTurns {
    waiting: RefCell<Vec<oneshot::Sender<()>>>,
    timer: Option<Box<dyn Fn(Duration) -> LocalBoxFuture<'static, ()>>>,
    interval: Duration,
    wake: Waker,
}

impl HostTurns {
    pub(crate) fn new(wake: Waker) -> Self {
        Self {
            waiting: RefCell::default(),
            timer: None,
            interval: Duration::ZERO,
            wake,
        }
    }

    pub(crate) fn with_timer<F: Future<Output = ()> + 'static>(
        mut self,
        interval: Duration,
        timer: impl Fn(Duration) -> F + 'static,
    ) -> Self {
        self.interval = interval;
        self.timer = Some(Box::new(move |duration| Box::pin(timer(duration))));
        self
    }

    /// The channel calls this only after Rust state confirms its payload was posted.
    /// Cancellation drops the timer and turn request, leaving payload ownership local.
    pub(crate) async fn receive<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(value) = poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await {
                return value;
            }
            if let Some(timer) = &self.timer {
                match select(future.as_mut(), timer(self.interval)).await {
                    Either::Left((value, _)) => return value,
                    Either::Right(_) => {}
                }
            }
            let (resume, resumed) = oneshot::channel();
            self.waiting.borrow_mut().push(resume);
            // A later sibling can enter guest code during this same root poll.
            // Interrupt it so the executor can perform the requested host turn.
            self.wake.wake_by_ref();
            resumed
                .await
                .expect("executor must resume host-turn waiters");
        }
    }

    pub(crate) fn has_waiters(&self) -> bool {
        self.waiting
            .borrow()
            .iter()
            .any(|waiter| !waiter.is_canceled())
    }

    pub(crate) fn resume(&self) {
        // Sending can reenter a child waker; release the borrow first.
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for waiter in waiting {
            let _ = waiter.send(());
        }
    }
}
