use crate::{channel, CancellableWorker, Receiver, ResourceError, Sender, Task, WorkerResource};
use futures_channel::oneshot;
use std::future::Future;

/// Owns a reusable execution thread. Tasks and resource channels are independent.
/// Keep this owner alive until its tasks have finished and released their resources.
/// Dropping it early can terminate a browser worker without running Rust destructors.
#[derive(Default)]
pub struct Worker {
    pub(crate) thread: web_thread::Thread,
}

impl Worker {
    pub fn new() -> Self {
        Self {
            thread: web_thread::Thread::new(),
        }
    }

    /// Constructs and runs a future on this worker. Only its factory and result
    /// cross threads; the future can hold non-Send objects across awaits.
    pub fn run<F: Future + 'static>(
        &self,
        code: impl FnOnce() -> F + Send + 'static,
    ) -> Task<F::Output>
    where
        F::Output: Send,
    {
        self.thread.run_send((), move |()| code())
    }

    /// Creates a channel whose receiver belongs to this worker. Awaiting setup
    /// registers browser delivery before returning. Move the receiver into a task
    /// with `run`, then send resources independently, including after startup.
    /// The worker must remain alive and yield to its host to deliver browser objects.
    pub async fn channel<R: WorkerResource>(
        &self,
    ) -> Result<(Sender<R>, Receiver<R>), ResourceError> {
        channel::on_worker(self).await
    }

    /// Starts a cooperatively cancellable task and transfers ownership of this
    /// worker to its stop/join handle. Configure resource channels before calling.
    pub fn run_cancellable<F: Future + 'static>(
        self,
        code: impl FnOnce(oneshot::Receiver<()>) -> F + Send + 'static,
    ) -> CancellableWorker<F::Output>
    where
        F::Output: Send,
    {
        CancellableWorker::new(self, code)
    }
}
