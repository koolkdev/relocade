use super::Runner;
use crate::worker::{wake_connection, WakeHost, WakePeer, Worker};
use crate::{
    timer_service, Clock, ClockSource, CpuHandle, ExecutionInterrupt, GuestTimeConfig, Result,
};
use futures_lite::future::or;
use futures_util::future::{select, Either};
use std::{
    future::{pending, Future},
    pin::pin,
};

#[cfg(test)]
mod tests;

/// Selects session timing and, optionally, supplies the host clock.
pub struct SessionConfig {
    guest: Option<GuestTimeConfig>,
    host: Option<Box<dyn ClockSource + Send>>,
}

impl SessionConfig {
    /// Use the host clock directly.
    pub fn host() -> Self {
        Self {
            guest: None,
            host: None,
        }
    }

    /// Drive guest hardware on its emulated clock using the platform's timing policy.
    pub fn guest(config: GuestTimeConfig) -> Self {
        Self {
            guest: Some(config),
            host: None,
        }
    }

    /// Use a clock supplied by the caller. The caller drives this source for the
    /// lifetime of the session. By default the worker starts its own timer service.
    pub fn with_host_clock(mut self, host: impl ClockSource + Send + 'static) -> Self {
        self.host = Some(Box::new(host));
        self
    }
}

/// Owns one CPU session and any host timer worker started for it.
/// Await `join`, `stop` or `run_until` through cleanup. Dropping a browser
/// worker before it finishes can skip Rust destructors on that worker.
pub struct ExecutionWorker<T: Send + 'static> {
    worker: Worker<Result<Option<T>>>,
    timers: Option<Worker<()>>,
    wake_host: WakeHost<ExecutionInterrupt>,
}

impl<T: Send + 'static> ExecutionWorker<T> {
    /// Starts execution on a new worker. The backend and device bindings are
    /// local to that worker and ready before the session callback runs.
    pub fn spawn(
        cpu: CpuHandle,
        config: SessionConfig,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> Self {
        let (wake_host, endpoint) = wake_connection(cpu.interrupt_handle());
        let (host, timers): (Box<dyn ClockSource + Send>, _) = match config.host {
            Some(host) => (host, None),
            None => {
                let (host, timers) = timer_service();
                (Box::new(host), Some(timers))
            }
        };
        let worker = Worker::spawn_with(cpu, move |cpu, mut stopped| async move {
            let initialize = async move {
                let host: Box<dyn ClockSource> = host;
                let clock = Clock::from_source(host.into());
                Runner::initialize(cpu, endpoint, clock, config.guest).await
            };
            let runner = match select(&mut stopped, pin!(initialize)).await {
                Either::Left(_) => return Ok(None),
                Either::Right((runner, _)) => runner?,
            };
            Ok(runner.run(stopped, session).await)
        });
        let timers = timers.map(|driver| driver.spawn_with_peer(wake_host.peer()));
        Self {
            worker,
            timers,
            wake_host,
        }
    }

    /// Captures the wake peer for another worker that can notify this session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake_host.peer()
    }

    /// Returns the session result on completion or `None` if `stop` wins.
    /// In either case, joins the execution worker and stops its owned timers.
    pub async fn run_until(mut self, stop: impl Future) -> Result<Option<T>> {
        let completed = or(
            async {
                stop.await;
                None
            },
            async { Some(self.worker.finished().await) },
        )
        .await;
        let cancelled = completed.is_none();
        let result = match completed {
            Some(result) => result,
            None => self.worker.stop().await,
        };
        if let Some(timers) = self.timers {
            timers.stop().await?;
        }
        let value = result?.transpose()?.flatten();
        Ok(if cancelled { None } else { value })
    }

    /// Requests cancellation, then waits for the session and its workers to finish.
    pub async fn stop(mut self) -> Result<Option<T>> {
        self.worker.request_stop();
        self.join().await
    }

    /// Waits for completion, including worker cleanup.
    pub async fn join(self) -> Result<Option<T>> {
        self.run_until(pending::<()>()).await
    }
}
