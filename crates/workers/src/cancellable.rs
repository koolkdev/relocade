use super::{Result, Task, Worker};
use futures_channel::oneshot;
use std::future::Future;

/// Owns a worker task and its cooperative cancellation signal.
/// The task must observe the supplied receiver and release its resources before
/// returning. Cancellation does not forcibly interrupt arbitrary blocking work.
///
/// Await [`Self::stop`] or [`Self::join`] before releasing the worker. Dropping
/// it early can terminate a browser thread before its Rust destructors run.
pub struct CancellableWorker<T: Send + 'static> {
    _worker: Worker,
    task: Task<T>,
    stop: Option<oneshot::Sender<()>>,
}

impl<T: Send + 'static> CancellableWorker<T> {
    pub(crate) fn new<F: Future<Output = T> + 'static>(
        worker: Worker,
        code: impl FnOnce(oneshot::Receiver<()>) -> F + Send + 'static,
    ) -> Self {
        let (stop, stopped) = oneshot::channel();
        let task = worker.run(move || code(stopped));
        Self {
            _worker: worker,
            task,
            stop: Some(stop),
        }
    }

    /// Requests cancellation and waits for the task to finish its cleanup.
    pub async fn stop(mut self) -> Result<T> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.join().await
    }

    /// Waits for ordinary completion without requesting cancellation.
    pub async fn join(mut self) -> Result<T> {
        self.finished().await
    }

    /// Waits for completion while retaining the owner, for example in a race
    /// with an external stop request. The wait can be dropped and resumed;
    /// consume its completed result only once.
    pub async fn finished(&mut self) -> Result<T> {
        (&mut self.task).await
    }
}

impl<T: Send + 'static> Drop for CancellableWorker<T> {
    fn drop(&mut self) {
        // Notify the task before releasing its thread.
        drop(self.stop.take());
    }
}
