//! Bounded execution of synchronous Rust jobs on supplied workers.

use crate::{jobs::Job, Error, JobExecutor, JobQueue, Thread};
use futures_channel::mpsc;
use futures_util::{
    future::{select, Either},
    stream::FuturesUnordered,
    StreamExt,
};
use std::fmt;

/// Owns a fixed set of caller-supplied threads and their task acknowledgements.
/// Poll [`Self::run`] alongside work submitted through the returned [`JobExecutor`].
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
            Self::Closed => f.write_str("job queue closed before the job completed"),
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
    pub fn new(threads: Vec<Thread>) -> Result<(Self, JobExecutor), PoolError> {
        if threads.is_empty() {
            return Err(PoolError::NoThreads);
        }
        let (queue, executor) = JobQueue::new();
        Ok((
            Self {
                threads,
                jobs: queue.jobs,
            },
            executor,
        ))
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
            active.push(job.on_thread(self.threads.pop().expect("an idle worker is available")));
        }
    }
}
