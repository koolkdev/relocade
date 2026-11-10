use super::{ChannelError, Error, Receiver, Sender, Transfer, Worker};
use futures_channel::oneshot;
use futures_util::future::LocalBoxFuture;
use std::{fmt, future::Future};

/// A worker's channel setup or task completion failed.
#[derive(Debug)]
pub enum WorkerError {
    Channel(ChannelError),
    Task(Error),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Channel(error) => error.fmt(f),
            Self::Task(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Channel(error) => Some(error),
            Self::Task(error) => Some(error),
        }
    }
}

impl From<ChannelError> for WorkerError {
    fn from(error: ChannelError) -> Self {
        Self::Channel(error)
    }
}

impl From<Error> for WorkerError {
    fn from(error: Error) -> Self {
        Self::Task(error)
    }
}

/// Owns a worker task and its cooperative cancellation signal.
/// The task must observe the supplied receiver and release its resources before
/// returning. Cancellation does not forcibly interrupt arbitrary blocking work.
///
/// Await [`Self::stop`] or [`Self::join`] before releasing the worker. Dropping
/// it early can terminate a browser thread before its Rust destructors run.
pub struct CancellableWorker<T: Send + 'static> {
    worker: Worker,
    task: Option<LocalBoxFuture<'static, Result<T, WorkerError>>>,
    setup_error: Option<ChannelError>,
    stop: Option<oneshot::Sender<()>>,
}

impl<T: Send + 'static> CancellableWorker<T> {
    /// Starts work immediately and retains its task through cleanup.
    /// The task treats either completion or closure of `stopped` as cancellation.
    /// The callback starts one task with [`Self::run`] or [`Self::listen`], then
    /// sends any initial messages. Setup failure requests cancellation; completion
    /// still waits for the task's cleanup before reporting that failure.
    pub fn spawn(
        start: impl FnOnce(&mut Self, oneshot::Receiver<()>) -> Result<(), ChannelError>,
    ) -> Self {
        let (stop, stopped) = oneshot::channel();
        let mut worker = Self {
            worker: Worker::new(),
            task: None,
            setup_error: None,
            stop: Some(stop),
        };
        if let Err(error) = start(&mut worker, stopped) {
            worker.setup_error = Some(error);
            worker.request_stop();
        } else {
            assert!(worker.task.is_some(), "start callback must submit a task");
        }
        worker
    }

    /// Starts this owner's single task and retains its completion.
    pub fn run<F: Future<Output = T> + 'static>(
        &mut self,
        code: impl FnOnce() -> F + Send + 'static,
    ) {
        assert!(self.task.is_none(), "worker task already started");
        let task = self.worker.run(code);
        self.task = Some(Box::pin(async move { Ok(task.await?) }));
    }

    /// Starts this owner's single task with an inbox and retains its completion
    /// before returning the sender. The task can propagate receive errors with `?`.
    pub fn listen<M: Transfer, F: Future<Output = Result<T, ChannelError>> + 'static>(
        &mut self,
        code: impl FnOnce(Receiver<M>) -> F + Send + 'static,
    ) -> Result<Sender<M>, ChannelError> {
        assert!(self.task.is_none(), "worker task already started");
        let (sender, task) = self.worker.listen(code)?;
        self.task = Some(Box::pin(async move { Ok(task.await??) }));
        Ok(sender)
    }

    /// Requests cooperative cancellation while retaining the worker.
    pub fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    /// Requests cancellation and waits for the task to finish its cleanup.
    pub async fn stop(mut self) -> Result<T, WorkerError> {
        self.request_stop();
        self.join().await
    }

    /// Waits for ordinary completion without requesting cancellation.
    pub async fn join(mut self) -> Result<T, WorkerError> {
        self.finished().await
    }

    /// Waits for completion while retaining the owner, for example in a race
    /// with an external stop request. The wait can be dropped and resumed;
    /// consume its completed result only once.
    pub async fn finished(&mut self) -> Result<T, WorkerError> {
        let result = match &mut self.task {
            Some(task) => Some(task.await),
            None => None,
        };
        if let Some(error) = self.setup_error.take() {
            return Err(error.into());
        }
        result.expect("worker completion already consumed")
    }
}

impl<T: Send + 'static> Drop for CancellableWorker<T> {
    fn drop(&mut self) {
        // Notify the task before releasing its thread.
        drop(self.stop.take());
    }
}
