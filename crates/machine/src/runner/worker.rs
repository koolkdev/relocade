use super::Runner;
use crate::worker::{
    self, wake_connection, SendTask, Thread, ThreadExt, WakeHost, WakePeer, WorkerResource,
};
use crate::{timer_service, Clock, CpuStorage, ExecutionInterrupt};
use futures_channel::oneshot;
use futures_lite::future::or;
use std::future::{pending, Future};

/// Owns a worker for one CPU session and its registered devices.
/// Use `runner.create_cpu` (or `Dos::new`) inside the supplied closure. CPU
/// storage is prepared before spawning; construction and access stay local.
///
/// Await `stop` or `join` before releasing the worker, just as with `Thread`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    _thread: Thread,
    task: SendTask<Option<T>>,
    stop: Option<oneshot::Sender<()>>,
    wake_host: WakeHost<ExecutionInterrupt>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    /// Captures the signal resource for another worker's startup. Bind the posted
    /// peer there before using senders that can wake this session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake_host.peer()
    }

    /// Runs a session with host-driven timers until it completes or `stop` resolves.
    /// Poll this on a responsive host, outside the execution worker. A stop signal
    /// returns `None` after dropping execution state on its owner; its value is
    /// ignored. Await this operation through completion for orderly cleanup.
    /// Timers are driven only for this call; use `spawn` and an external timer
    /// service when clock handles must remain usable after the session ends.
    pub async fn run_until<R: WorkerResource>(
        stop: impl Future,
        resource: R,
        session: impl AsyncFnOnce(&Runner, R) -> T + Send + 'static,
    ) -> worker::Result<Option<T>>
    where
        (CpuStorage, R): WorkerResource,
    {
        let (clock, timers) = timer_service();
        let mut worker = Self::spawn(clock, resource, session);
        let execution = async move {
            let completed = or(
                async {
                    stop.await;
                    None
                },
                async { Some((&mut worker.task).await) },
            )
            .await;
            match completed {
                Some(result) => result,
                None => worker.stop().await.map(|_| None),
            }
        };
        or(execution, async {
            timers.run().await;
            pending().await
        })
        .await
    }

    /// Posts session resources together with the prepared CPU storage. Pass `()`
    /// when startup captures contain only ordinary Send Rust values.
    pub fn spawn<C: Clock + Send + 'static, R: WorkerResource>(
        clock: C,
        resource: R,
        session: impl AsyncFnOnce(&Runner<C>, R) -> T + Send + 'static,
    ) -> Self
    where
        (CpuStorage, R): WorkerResource,
    {
        let storage = CpuStorage::new().expect("allocate execution worker CPU storage");
        let (wake_host, endpoint) = wake_connection(storage.interrupt_handle());
        let thread = Thread::new();
        let (stop, stopped) = oneshot::channel();
        let task =
            thread.run_resource((storage, resource), move |(storage, resource)| async move {
                Runner::new(clock, storage, endpoint)
                    .run(stopped, async move |runner| session(runner, resource).await)
                    .await
            });
        Self {
            _thread: thread,
            task,
            stop: Some(stop),
            wake_host,
        }
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(mut self) -> worker::Result<Option<T>> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.join().await
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(mut self) -> worker::Result<Option<T>> {
        (&mut self.task).await
    }
}

impl<T: Send + 'static> Drop for ExecutionWorker<T> {
    fn drop(&mut self) {
        // Closing the stop channel also wakes the session before releasing its thread.
        drop(self.stop.take());
    }
}
