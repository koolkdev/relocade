use super::Runner;
use crate::worker::{self, SendTask, Thread};
use crate::Clock;
use futures_util::future::{AbortHandle, Abortable};

/// Owns a worker for one CPU session and its registered devices.
/// Construct the CPU and other non-Send state inside the supplied closure.
///
/// Await `stop` or `join` before releasing the worker, just as with `Thread`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    _thread: Thread,
    task: SendTask<Option<T>>,
    stop: AbortHandle,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    pub fn spawn<C: Clock + Send + 'static>(
        clock: C,
        session: impl AsyncFnOnce(&Runner<C>) -> T + Send + 'static,
    ) -> Self {
        let thread = Thread::new();
        let (stop, registration) = AbortHandle::new_pair();
        let task = thread.run_send((), move |()| async move {
            // Cancellation drops the runner on its owner before task completion.
            Abortable::new(Runner::with_clock(clock).run(session), registration)
                .await
                .ok()
        });
        Self {
            _thread: thread,
            task,
            stop,
        }
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(self) -> worker::Result<Option<T>> {
        self.stop.abort();
        self.join().await
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(mut self) -> worker::Result<Option<T>> {
        (&mut self.task).await
    }
}

impl<T: Send + 'static> Drop for ExecutionWorker<T> {
    fn drop(&mut self) {
        // Wake an abandoned session before releasing its thread handle.
        self.stop.abort();
    }
}
