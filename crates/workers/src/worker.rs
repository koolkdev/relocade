use super::{ChannelError, Error, Receiver, Thread, Transfer};
use futures_channel::oneshot;
use futures_util::future::{select, Either, LocalBoxFuture};
use std::{fmt, future::Future, pin::pin};

/// A worker's startup or task completion failed.
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
pub struct Worker<T: Send + 'static> {
    _thread: Thread,
    task: LocalBoxFuture<'static, Result<Option<T>, WorkerError>>,
    stop: Option<oneshot::Sender<()>>,
}

impl<T: Send + 'static> Worker<T> {
    /// Starts work immediately and retains its task through cleanup.
    /// The task treats either completion or closure of `stopped` as cancellation.
    pub fn spawn<F: Future<Output = T> + 'static>(
        code: impl FnOnce(oneshot::Receiver<()>) -> F + Send + 'static,
    ) -> Self {
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let task = thread.run(move || code(stopped));
        Self {
            _thread: thread,
            task: Box::pin(async move { Ok(Some(task.await?)) }),
            stop: Some(stop),
        }
    }

    /// Transfers one initial value before invoking the task with its cancellation
    /// receiver. This uses [`Thread::listen`] and the ordinary typed channel;
    /// the startup inbox is released before the task runs.
    ///
    /// Startup observes cancellation while waiting for the value. Transfer failure
    /// requests cancellation and waits for worker cleanup before reporting the error.
    pub fn spawn_with<M: Transfer, F: Future<Output = T> + 'static>(
        initial: M,
        code: impl FnOnce(M, oneshot::Receiver<()>) -> F + Send + 'static,
    ) -> Self {
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let mut stop = Some(stop);
        let task: LocalBoxFuture<'static, Result<Option<T>, WorkerError>> =
            match thread.listen(move |input| start(input, stopped, code)) {
                Ok((mut input, task)) => {
                    let sent = input.send(initial);
                    if sent.is_err() {
                        let _ = stop.take().unwrap().send(());
                    }
                    Box::pin(async move {
                        let result = task.await;
                        // Even a failed send must retain the thread until cleanup.
                        sent?;
                        Ok(result??)
                    })
                }
                Err(error) => Box::pin(async move { Err(error.into()) }),
            };
        Self {
            _thread: thread,
            task,
            stop,
        }
    }

    /// Requests cooperative cancellation while retaining the worker.
    pub fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    /// Requests cancellation and waits for the task to finish its cleanup.
    pub async fn stop(mut self) -> Result<Option<T>, WorkerError> {
        self.request_stop();
        self.join().await
    }

    /// Waits for ordinary completion without requesting cancellation.
    pub async fn join(mut self) -> Result<Option<T>, WorkerError> {
        self.finished().await
    }

    /// Waits for completion while retaining the owner, for example in a race
    /// with an external stop request. The wait can be dropped and resumed;
    /// consume its completed result only once.
    /// `None` means startup was stopped before the task received its initial value.
    pub async fn finished(&mut self) -> Result<Option<T>, WorkerError> {
        self.task.as_mut().await
    }
}

impl<T: Send + 'static> Drop for Worker<T> {
    fn drop(&mut self) {
        // Notify the task before releasing its thread.
        drop(self.stop.take());
    }
}

async fn start<M: Transfer, F: Future>(
    mut input: Receiver<M>,
    mut stopped: oneshot::Receiver<()>,
    code: impl FnOnce(M, oneshot::Receiver<()>) -> F,
) -> Result<Option<F::Output>, ChannelError> {
    let received = {
        let receive = pin!(input.recv());
        match select(&mut stopped, receive).await {
            Either::Left(_) => return Ok(None),
            Either::Right((received, _)) => received,
        }
    };
    let value = received.ok_or(ChannelError::Closed)??;
    drop(input);
    Ok(Some(code(value, stopped).await))
}
