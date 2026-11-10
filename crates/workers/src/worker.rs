use crate::{channel, ChannelError, Receiver, SendTask, Sender, Thread, Transfer};
use std::future::Future;

/// Owns a reusable thread. Tasks and message delivery have separate entrypoints.
/// Await every task before releasing this owner so worker-local cleanup can finish.
#[derive(Default)]
pub struct Worker {
    thread: Thread,
}

impl Worker {
    pub fn new() -> Self {
        Self {
            thread: Thread::new(),
        }
    }

    /// Constructs a future on this worker. Its factory and result cross threads;
    /// the future itself may retain non-Send values across awaits.
    pub fn run<F: Future + 'static>(
        &self,
        code: impl FnOnce() -> F + Send + 'static,
    ) -> SendTask<F::Output>
    where
        F::Output: Send,
    {
        self.thread.run_send((), move |()| code())
    }

    /// Starts a task with a local inbox and returns its sender immediately.
    /// Values can be sent before startup finishes and throughout the task's life.
    /// Inbox binding happens on this worker; callers only send complete values.
    ///
    /// ```no_run
    /// use relocade_workers::{Receiver, Worker};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let worker = Worker::new();
    /// let (mut input, task) = worker.listen(|mut input: Receiver<String>| async move {
    ///     input.recv().await.transpose()
    /// })?;
    /// input.send("ready".to_owned())?;
    /// assert_eq!(task.await??, Some("ready".to_owned()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn listen<T: Transfer, F: Future + 'static>(
        &self,
        code: impl FnOnce(Receiver<T>) -> F + Send + 'static,
    ) -> Result<(Sender<T>, SendTask<F::Output>), ChannelError>
    where
        F::Output: Send,
    {
        channel::on_worker(&self.thread, code)
    }
}
