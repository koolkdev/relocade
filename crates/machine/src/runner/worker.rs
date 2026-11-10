use super::{GuestTimeConfig, Runner};
use crate::worker::{
    wake_connection, CancellableWorker, ResourceError, WakeEndpoint, WakeHost, WakePeer, Worker,
};
use crate::{timer_service, Clock, CpuStorage, ExecutionInterrupt, TimerHandle};
use futures_lite::future::or;
use std::future::Future;

/// Owns a worker for one CPU session and its registered devices.
/// Use `runner.create_cpu` (or `Dos::new`) inside the supplied closure. CPU
/// storage is prepared before spawning; construction and access stay local.
///
/// Await `stop` or `join` before releasing the worker, just as with `Worker`.
/// Dropping the handle or its completion future early can terminate the underlying
/// browser worker before Rust destructors run.
pub struct ExecutionWorker<T: Send + 'static> {
    worker: CancellableWorker<Result<Option<T>, ResourceError>>,
    wake_host: WakeHost<ExecutionInterrupt>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    /// Captures the signal resource for another worker. Bind the received
    /// peer there before using senders that can wake this session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake_host.peer()
    }

    /// Runs a session with a dedicated timer worker until it completes or `stop` resolves.
    /// Poll this outside the execution worker. A stop signal
    /// returns `None` after dropping execution state on its owner; its value is
    /// ignored. Await this operation through completion for orderly cleanup.
    /// Timers are driven only for this call; use `spawn` and an external timer
    /// service when clock handles must remain usable after the session ends.
    pub async fn run_until(
        stop: impl Future,
        session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
    ) -> Result<Option<T>, ResourceError> {
        let (handle, timers) = timer_service();
        let mut execution = Self::spawn(handle, session).await?;
        let timers = match timers.spawn_with_peer(execution.wake_peer()).await {
            Ok(timers) => timers,
            Err(error) => {
                execution.stop().await?;
                return Err(error);
            }
        };
        let completed = or(
            async {
                stop.await;
                None
            },
            async {
                let result = execution.worker.finished().await;
                Some(result.map_err(ResourceError::from).flatten())
            },
        )
        .await;
        let result = match completed {
            Some(result) => result,
            None => execution.stop().await.map(|_| None),
        };
        timers.stop().await??;
        result
    }

    pub async fn spawn(
        host_timers: TimerHandle,
        session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
    ) -> Result<Self, ResourceError> {
        Self::spawn_with_clock(move || host_timers.into(), session).await
    }

    /// Constructs a clock on the execution owner before starting the session.
    /// A transferable factory can supply a custom clock on that owner.
    /// Use [`Self::spawn_with_guest_time`] for emulated guest time.
    pub async fn spawn_with_clock(
        make_clock: impl FnOnce() -> Clock + Send + 'static,
        session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
    ) -> Result<Self, ResourceError> {
        Self::spawn_runner(
            move |storage, endpoint| Runner::new(make_clock(), storage, endpoint),
            session,
        )
        .await
    }

    /// Constructs guest-time configuration on the execution owner, then runs
    /// the session. The factory can create a local host clock for pacing execution slices.
    /// Devices keep the existing [`Clock`] interface.
    pub async fn spawn_with_guest_time(
        make_time: impl FnOnce() -> GuestTimeConfig + Send + 'static,
        session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
    ) -> Result<Self, ResourceError> {
        Self::spawn_runner(
            move |storage, endpoint| Runner::with_guest_time(make_time(), storage, endpoint),
            session,
        )
        .await
    }

    async fn spawn_runner(
        runner: impl FnOnce(CpuStorage, WakeEndpoint<ExecutionInterrupt>) -> Runner + Send + 'static,
        session: impl AsyncFnOnce(&Runner) -> T + Send + 'static,
    ) -> Result<Self, ResourceError> {
        let storage = CpuStorage::new().expect("allocate execution worker CPU storage");
        let (wake_host, endpoint) = wake_connection(storage.interrupt_handle());
        let worker = Worker::new();
        let (mut sender, mut incoming) = worker.channel::<CpuStorage>().await?;
        let worker = worker.run_cancellable(move |stopped| async move {
            let Some(storage) = incoming.recv().await else {
                return Ok(None);
            };
            let storage = storage?;
            drop(incoming);
            Ok(runner(storage, endpoint).run(stopped, session).await)
        });
        if let Err(error) = sender.send(storage) {
            drop(sender);
            let _ = worker.stop().await;
            return Err(error);
        }
        Ok(Self { worker, wake_host })
    }

    /// Stops even while a service is pending, drops the session on its owner,
    /// then waits for task completion before releasing the thread.
    pub async fn stop(self) -> Result<Option<T>, ResourceError> {
        self.worker.stop().await?
    }

    /// Waits for ordinary completion. `None` denotes an explicit stop.
    pub async fn join(self) -> Result<Option<T>, ResourceError> {
        self.worker.join().await?
    }
}
