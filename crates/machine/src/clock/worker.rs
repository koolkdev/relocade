use crate::worker::{self, SendTask, Thread};
use futures_channel::oneshot;

/// Owns the timer thread until it has released its requests and waiters.
/// Await [`Self::stop`] or [`Self::join`] before dropping the worker. Dropping
/// it early can terminate a browser thread before its Rust destructors run.
pub struct TimerWorker {
    _thread: Thread,
    task: SendTask<()>,
    stop: Option<oneshot::Sender<()>>,
}

impl TimerWorker {
    pub(super) fn spawn(
        start: impl FnOnce(&Thread, oneshot::Receiver<()>) -> SendTask<()>,
    ) -> Self {
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let task = start(&thread, stopped);
        Self {
            _thread: thread,
            task,
            stop: Some(stop),
        }
    }

    /// Cancels pending timers and waits for their worker to exit.
    /// Release timer waiters before stopping their service.
    pub async fn stop(mut self) -> worker::Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.join().await
    }

    /// Waits until every timer handle and pending wait has been released.
    pub async fn join(mut self) -> worker::Result<()> {
        (&mut self.task).await
    }
}

impl Drop for TimerWorker {
    fn drop(&mut self) {
        drop(self.stop.take());
    }
}
