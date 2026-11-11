use super::Runner;
use crate::worker::{wake_connection, WakeEndpoint, WakeHost, WakePeer, Worker};
use crate::{CpuHandle, ExecutionInterrupt, Result};
use futures_lite::future::or;
use futures_util::future::{select, Either};
use std::{future::Future, pin::pin};

#[cfg(test)]
mod tests;

/// Owns a worker for one CPU session and its registered devices.
/// Created by consuming a [`CpuHandle`]. Use `runner.cpu().io().connect` or `Dos::new`
/// inside the callback to attach local devices. The CPU backend is ready before
/// the callback starts; construction failures are returned by `join` or `stop`.
///
/// Await `stop` or `join` before releasing the worker, just as with `Thread`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    worker: Worker<Result<Option<T>>>,
    wake_host: WakeHost<ExecutionInterrupt>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    /// Captures the signal resource for another worker's startup. Bind the posted
    /// peer there before using senders that can wake this session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake_host.peer()
    }

    pub(crate) async fn run_until(mut self, stop: impl Future) -> Result<Option<T>> {
        let completed = or(
            async {
                stop.await;
                None
            },
            async { Some(self.worker.finished().await) },
        )
        .await;
        match completed {
            Some(result) => result?.transpose().map(Option::flatten),
            None => self.stop().await.map(|_| None),
        }
    }

    pub(crate) fn spawn(
        cpu: CpuHandle,
        make_runner: impl AsyncFnOnce(CpuHandle, WakeEndpoint<ExecutionInterrupt>) -> Result<Runner>
            + Send
            + 'static,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> Self {
        let (wake_host, endpoint) = wake_connection(cpu.interrupt_handle());
        let worker = Worker::spawn_with(cpu, move |cpu, mut stopped| async move {
            let runner = {
                let initialize = pin!(make_runner(cpu, endpoint));
                match select(&mut stopped, initialize).await {
                    Either::Left(_) => return Ok(None),
                    Either::Right((runner, _)) => runner?,
                }
            };
            Ok(runner.run(stopped, session).await)
        });
        Self { worker, wake_host }
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(self) -> Result<Option<T>> {
        self.worker.stop().await?.transpose().map(Option::flatten)
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(self) -> Result<Option<T>> {
        self.worker.join().await?.transpose().map(Option::flatten)
    }
}
