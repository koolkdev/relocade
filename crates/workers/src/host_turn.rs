//! Requests to return through the host event loop once, then resume Rust work.

use futures_channel::oneshot;
use futures_util::future::{select, Either, LocalBoxFuture};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    task::{Poll, Waker},
    time::Duration,
};

pub(super) struct HostTurns {
    waiting: RefCell<Vec<oneshot::Sender<()>>>,
    timer: Option<Box<dyn Fn(Duration) -> LocalBoxFuture<'static, ()>>>,
    wake: Waker,
}

impl HostTurns {
    pub(super) fn new(wake: Waker) -> Self {
        Self {
            waiting: RefCell::default(),
            timer: None,
            wake,
        }
    }

    pub(super) fn with_timer<F: Future<Output = ()> + 'static>(
        mut self,
        timer: impl Fn(Duration) -> F + 'static,
    ) -> Self {
        self.timer = Some(Box::new(move |duration| Box::pin(timer(duration))));
        self
    }

    /// Used only after Rust communication confirms an engine object was posted.
    /// The wait owns its timer and turn request, so cancellation releases both.
    pub(super) async fn receive<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(result) =
                std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
            {
                return result;
            }
            if let Some(timer) = &self.timer {
                match select(future.as_mut(), timer(Duration::from_millis(1))).await {
                    Either::Left((result, _)) => return result,
                    Either::Right(_) => {}
                }
            }
            self.request().await.expect("executor resumes host receipt");
        }
    }

    pub(super) fn request(&self) -> oneshot::Receiver<()> {
        let (resume, resumed) = oneshot::channel();
        self.waiting.borrow_mut().push(resume);
        // A later sibling may enter synchronous guest execution in this poll.
        // Interrupt it so the executor can return and perform the requested turn.
        self.wake.wake_by_ref();
        resumed
    }

    pub(super) fn has_waiters(&self) -> bool {
        self.waiting
            .borrow()
            .iter()
            .any(|waiter| !waiter.is_canceled())
    }

    pub(super) fn resume(&self) {
        // A child waker can reenter the owner. Release the borrow before sending.
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for waiter in waiting {
            let _ = waiter.send(());
        }
    }
}
