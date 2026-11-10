use super::{ChannelError, Error, Receiver, Sender, Thread, Transfer};
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
    thread: Thread,
    task: Option<LocalBoxFuture<'static, Result<Option<T>, WorkerError>>>,
    setup_error: Option<ChannelError>,
    stop: Option<oneshot::Sender<()>>,
    stopped: Option<oneshot::Receiver<()>>,
}

impl<T: Send + 'static> Worker<T> {
    /// Starts work immediately and retains its task through cleanup.
    /// The task treats either completion or closure of `stopped` as cancellation.
    /// The callback starts one task with [`Self::run`] or [`Self::listen`], then
    /// sends any initial messages. Setup failure requests cancellation; completion
    /// still waits for the task's cleanup before reporting that failure.
    pub fn spawn(start: impl FnOnce(&mut Self) -> Result<(), ChannelError>) -> Self {
        let (stop, stopped) = oneshot::channel();
        let mut worker = Self {
            thread: Thread::new(),
            task: None,
            setup_error: None,
            stop: Some(stop),
            stopped: Some(stopped),
        };
        if let Err(error) = start(&mut worker) {
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
        code: impl FnOnce(oneshot::Receiver<()>) -> F + Send + 'static,
    ) {
        let stopped = self.stopped.take().expect("worker task already started");
        let task = self.thread.run(move || code(stopped));
        self.task = Some(Box::pin(async move { Ok(Some(task.await?)) }));
    }

    /// Waits for an initial message, then runs this owner's single task.
    /// Startup observes stop before invoking the task and releases its inbox
    /// before handing over the complete value and cancellation receiver.
    /// Completion is retained before the sender is returned.
    pub fn listen<M: Transfer, F: Future<Output = T> + 'static>(
        &mut self,
        code: impl FnOnce(M, oneshot::Receiver<()>) -> F + Send + 'static,
    ) -> Result<Sender<M>, ChannelError> {
        let stopped = self.stopped.take().expect("worker task already started");
        let (sender, task) = self
            .thread
            .listen(move |input| start(input, stopped, code))?;
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
