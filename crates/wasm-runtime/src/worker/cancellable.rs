use super::{Result, SendTask, Thread};
use futures_channel::oneshot;

/// Owns a worker task and its cooperative cancellation signal.
/// The task must observe the supplied receiver and release its resources before
/// returning. Cancellation does not forcibly interrupt arbitrary blocking work.
///
/// Await [`Self::stop`] or [`Self::join`] before releasing the worker. Dropping
/// it early can terminate a browser thread before its Rust destructors run.
pub struct CancellableWorker<T: Send + 'static> {
    _thread: Thread,
    task: SendTask<T>,
    stop: Option<oneshot::Sender<()>>,
}

impl<T: Send + 'static> CancellableWorker<T> {
    /// Starts one task using the thread's existing posting or resource transport.
    /// The task treats either completion or closure of `stopped` as cancellation.
    pub fn spawn(start: impl FnOnce(&Thread, oneshot::Receiver<()>) -> SendTask<T>) -> Self {
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let task = start(&thread, stopped);
        Self {
            _thread: thread,
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
