use super::{ExecutionSchedule, Runner};
use crate::worker::{self, wake_connection, WakeHost, WakePeer, Worker};
use crate::{CpuHandle, ExecutionInterrupt};
use futures_lite::future::or;
use std::future::Future;

/// Owns a worker for one CPU session and its registered devices.
/// Created by consuming a [`CpuHandle`]. Use `runner.attach_io` or `Dos::new`
/// inside the callback to construct local devices and the backend over that CPU.
///
/// Await `stop` or `join` before releasing the worker, just as with `Thread`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    worker: Worker<Option<T>>,
    wake_host: WakeHost<ExecutionInterrupt>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    /// Captures the signal resource for another worker's startup. Bind the posted
    /// peer there before using senders that can wake this session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake_host.peer()
    }

    pub(crate) async fn run_until(
        mut self,
        stop: impl Future,
    ) -> Result<Option<T>, worker::WorkerError> {
        let completed = or(
            async {
                stop.await;
                None
            },
            async { Some(self.worker.finished().await.map(Option::flatten)) },
        )
        .await;
        match completed {
            Some(result) => result,
            None => self.stop().await.map(|_| None),
        }
    }

    pub(crate) fn spawn(
        cpu: CpuHandle,
        schedule: impl FnOnce() -> ExecutionSchedule + Send + 'static,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> Self {
        let (wake_host, endpoint) = wake_connection(cpu.interrupt_handle());
        let worker = Worker::spawn_with(cpu, move |cpu, stopped| async move {
            Runner::with_schedule(schedule(), cpu, endpoint)
                .run(stopped, session)
                .await
        });
        Self { worker, wake_host }
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(self) -> Result<Option<T>, worker::WorkerError> {
        self.worker.stop().await.map(Option::flatten)
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(self) -> Result<Option<T>, worker::WorkerError> {
        self.worker.join().await.map(Option::flatten)
    }
}
