use crate::{channel, ChannelError, Receiver, SendTask, Sender, Transfer};
use std::future::Future;

/// Owns a reusable thread. Tasks and message delivery have separate entrypoints.
/// Await every task before releasing this owner so worker-local cleanup can finish.
#[derive(Default)]
pub struct Thread {
    inner: web_thread::Thread,
}

impl Thread {
    pub fn new() -> Self {
        Self {
            inner: web_thread::Thread::new(),
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
        self.inner.run_send((), move |()| code())
    }

    /// Starts a task with a local inbox and returns its sender immediately.
    /// Values can be sent before startup finishes and throughout the task's life.
    /// Inbox binding happens on this worker; callers only send complete values.
    ///
    /// ```no_run
    /// use relocade_workers::{ChannelError, Receiver, Thread};
    /// async fn receive(mut input: Receiver<String>) -> Result<Option<String>, ChannelError> {
    ///     input.recv().await.transpose()
    /// }
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let thread = Thread::new();
    /// let (mut input, task) = thread.listen(receive)?;
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
        channel::on_thread(&self.inner, code)
    }
}
