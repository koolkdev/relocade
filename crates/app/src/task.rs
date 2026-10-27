use futures_util::future::{AbortHandle, Abortable};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use web_thread::{Post, SendTask, Thread};

/// An application's own result, or cancellation before it completed.
#[derive(Debug, Eq, PartialEq)]
pub enum Exit<T> {
    Completed(T),
    Cancelled,
}

/// Keeps a worker alive until its accepted task completes.
/// Request shutdown with `stop`, then await this value before dropping it.
#[must_use = "keep the task alive and await its completion"]
pub struct Task<T: Send> {
    worker: Option<Thread>,
    task: SendTask<Result<Exit<T>, String>>,
    stop: AbortHandle,
}

impl<T: Send + 'static> Task<T> {
    pub(crate) fn spawn<C: Post + 'static, F>(
        context: C,
        run: impl FnOnce(C, AbortHandle) -> F + Send + 'static,
    ) -> Self
    where
        F: Future<Output = Result<Exit<T>, String>> + 'static,
    {
        let worker = Thread::new();
        let (stop, registration) = AbortHandle::new_pair();
        let task = worker.run_send(context, move |context| async move {
            let cancellation = registration.handle();
            Abortable::new(run(context, cancellation), registration)
                .await
                .unwrap_or(Ok(Exit::Cancelled))
        });
        Self {
            worker: Some(worker),
            task,
            stop,
        }
    }

    /// Wakes pending work. Always-ready tasks must also check cancellation.
    pub fn stop(&self) {
        self.stop.abort();
    }
}

impl<T: Send> Future for Task<T> {
    type Output = Result<Exit<T>, String>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let result = std::task::ready!(Pin::new(&mut self.task).poll(context));
        self.worker.take();
        Poll::Ready(
            result
                .map_err(|error| error.to_string())
                .and_then(|result| result),
        )
    }
}
