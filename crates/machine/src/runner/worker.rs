use super::Runner;
use crate::worker::{self, SendTask, Thread};
use futures_channel::oneshot;
use futures_util::future::{select, Either};
use std::{future::Future, pin::pin};

/// Owns an emulation thread and its local session task using the existing worker
/// transport. Construct CPUs and other non-Send state inside the supplied closure.
///
/// Await `stop` or `join` before releasing the worker, just as with `Thread`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    thread: Thread,
    task: SendTask<Option<T>>,
    stop: oneshot::Sender<()>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    pub fn spawn<F: Future<Output = T> + 'static>(
        session: impl FnOnce(Runner) -> F + Send + 'static,
    ) -> Self {
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let task = thread.run_send((), move |()| async move {
            let session = pin!(session(Runner::default()));
            match select(stopped, session).await {
                Either::Left(_) => None,
                Either::Right((result, _)) => Some(result),
            }
            // The losing session future is dropped here, on its owning worker,
            // before completion is sent back to the caller.
        });
        Self { thread, task, stop }
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(self) -> worker::Result<Option<T>> {
        let Self { thread, task, stop } = self;
        let _ = stop.send(());
        let result = task.await;
        drop(thread);
        result
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(self) -> worker::Result<Option<T>> {
        let Self { thread, task, stop } = self;
        let result = task.await;
        drop(stop);
        drop(thread);
        result
    }
}
