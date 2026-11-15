//! Synchronous calculations submitted as owned Rust closures.

use crate::{Error, PoolError, Thread};
use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{select, Either},
    StreamExt,
};
use std::{future::Future, pin::pin};

/// Receives jobs for sequential execution on its current thread.
/// A pool uses the same submissions to distribute jobs over supplied threads.
pub struct JobQueue {
    pub(crate) jobs: mpsc::UnboundedReceiver<Job>,
}

/// Sendable job submission with owned inputs and asynchronous results.
/// A dedicated queue runs one job at a time; a pool bounds concurrent jobs by
/// its worker count. Callers control how many jobs they leave outstanding.
#[derive(Clone)]
pub struct JobExecutor {
    jobs: mpsc::UnboundedSender<Job>,
}

impl JobQueue {
    pub fn new() -> (Self, JobExecutor) {
        let (sender, jobs) = mpsc::unbounded();
        (Self { jobs }, JobExecutor { jobs: sender })
    }

    /// Runs jobs until stopped or all submission handles are dropped.
    /// Stop closes admission, then finishes accepted work before returning.
    /// Each job must finish on its own; cancellation does not interrupt a calculation.
    pub async fn run(mut self, stopped: impl Future) {
        let mut stopped = pin!(stopped);
        loop {
            match select(stopped.as_mut(), self.jobs.next()).await {
                Either::Left(_) | Either::Right((None, _)) => break,
                Either::Right((Some(job), _)) => job.run(),
            }
        }
        self.jobs.close();
        while let Some(job) = self.jobs.next().await {
            job.run();
        }
    }
}

impl JobExecutor {
    /// Queues a job on the first poll. Dropping the returned future discards its
    /// result; accepted work still owns its inputs until completion.
    /// Jobs must finish without waiting on other work from the same queue.
    pub async fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, PoolError> {
        let (value, result) = oneshot::channel();
        let (finished, completion) = oneshot::channel();
        self.jobs
            .unbounded_send(Job {
                code: Box::new(move || {
                    let _ = value.send(job());
                }),
                finished,
            })
            .map_err(|_| PoolError::Closed)?;
        completion
            .await
            .map_err(|_| PoolError::Closed)?
            .map_err(PoolError::Worker)?;
        result.await.map_err(|_| PoolError::Closed)
    }
}

pub(crate) struct Job {
    code: Box<dyn FnOnce() + Send>,
    finished: oneshot::Sender<Result<(), Error>>,
}

impl Job {
    fn run(self) {
        (self.code)();
        let _ = self.finished.send(Ok(()));
    }

    pub(crate) async fn on_thread(self, thread: Thread) -> Option<Thread> {
        let task = thread.run(move || async move { (self.code)() });
        let result = task.await;
        let reusable = result.is_ok();
        let _ = self.finished.send(result);
        reusable.then_some(thread)
    }
}
