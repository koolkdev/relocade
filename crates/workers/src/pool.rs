//! Bounded execution of synchronous Rust jobs on supplied workers.

use crate::{Error, Thread};
use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{select, Either},
    stream::FuturesUnordered,
    StreamExt,
};
use std::fmt;

/// Owns a fixed set of caller-supplied threads and their task acknowledgements.
/// Poll [`Self::run`] alongside work submitted through the returned [`PoolHandle`].
/// Handles may move between threads; this driver stays on its browser owner.
///
/// Drop all handles, then await the driver to drain accepted jobs before shutdown.
/// Dropping the driver early can terminate browser workers before Rust cleanup.
///
/// ```no_run
/// use futures_util::future::join;
/// use relocade_workers::{Pool, PoolError, Thread};
/// # async fn example(threads: Vec<Thread>) -> Result<(), PoolError> {
/// let (pool, jobs) = Pool::new(threads)?;
/// let (_, result) = join(pool.run(), async move {
///     jobs.execute(|| 6 * 7).await
/// }).await;
/// assert_eq!(result?, 42);
/// # Ok(())
/// # }
/// ```
pub struct Pool {
    threads: Vec<Thread>,
    jobs: mpsc::UnboundedReceiver<Job>,
}

/// A sendable submission handle sharing the pool's concurrency limit.
/// Running jobs are bounded by the supplied thread count; queued jobs own their
/// inputs and are admitted in submission order as workers become available.
#[derive(Clone)]
pub struct PoolHandle {
    jobs: mpsc::UnboundedSender<Job>,
}

/// Failure to create a pool or execute a submitted job.
#[derive(Debug)]
pub enum PoolError {
    NoThreads,
    Closed,
    Worker(Error),
}

impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoThreads => f.write_str("a worker pool requires at least one thread"),
            Self::Closed => f.write_str("worker pool closed before the job completed"),
            Self::Worker(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PoolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Worker(error) => Some(error),
            _ => None,
        }
    }
}

impl Pool {
    /// Takes ownership of existing workers without creating additional threads.
    pub fn new(threads: Vec<Thread>) -> Result<(Self, PoolHandle), PoolError> {
        if threads.is_empty() {
            return Err(PoolError::NoThreads);
        }
        let (sender, jobs) = mpsc::unbounded();
        Ok((Self { threads, jobs }, PoolHandle { jobs: sender }))
    }

    /// Runs at most one job on each supplied worker. Completion releases a slot
    /// only after the worker acknowledges the task, even if its waiter was dropped.
    /// After the last handle is dropped, drains accepted jobs before returning.
    /// Failed workers are retired. If none remain, returns and rejects queued
    /// jobs with [`PoolError::Closed`].
    pub async fn run(mut self) {
        let mut active = FuturesUnordered::new();
        loop {
            if self.threads.is_empty() {
                match active.next().await {
                    Some(Some(thread)) => self.threads.push(thread),
                    Some(None) => {}
                    // All workers failed; closing the queue rejects remaining jobs.
                    None => return,
                }
                continue;
            }
            let job = if active.is_empty() {
                self.jobs.next().await
            } else {
                match select(self.jobs.next(), active.next()).await {
                    Either::Left((job, _)) => job,
                    Either::Right((thread, _)) => {
                        if let Some(Some(thread)) = thread {
                            self.threads.push(thread);
                        }
                        continue;
                    }
                }
            };
            let Some(job) = job else {
                while active.next().await.is_some() {}
                return;
            };
            active.push(job.run(self.threads.pop().expect("an idle worker is available")));
        }
    }
}

impl PoolHandle {
    /// Queues a job on the first poll. Dropping the returned future discards its
    /// result; accepted work still owns its inputs and occupies its worker slot.
    /// Jobs must finish without awaiting other work from this same pool.
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

struct Job {
    code: Box<dyn FnOnce() + Send>,
    finished: oneshot::Sender<Result<(), Error>>,
}

impl Job {
    async fn run(self, thread: Thread) -> Option<Thread> {
        let task = thread.run(move || async move { (self.code)() });
        let result = task.await;
        let reusable = result.is_ok();
        let _ = self.finished.send(result);
        reusable.then_some(thread)
    }
}

#[cfg(feature = "compiler")]
impl relocade_compiler::CompileExecutor for PoolHandle {
    type Error = PoolError;

    fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> impl std::future::Future<Output = Result<T, Self::Error>> {
        self.execute(job)
    }
}
