use super::Runner;
use crate::worker::{wake_connection, WakeEndpoint, WakeHost, WakePeer, Worker};
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

    /// Use an emulated guest clock. Without a host clock, guest time is unpaced.
    pub fn guest(config: GuestTimeConfig) -> Self {
        Self {
            guest: Some(config),
            host: None,
        }
    }

    /// Use a caller-driven host clock. For a guest session this enables pacing.
    /// A host session starts its own timer service if no clock is supplied.
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
        if let Some(guest) = config.guest {
            return Self::spawn_with(
                cpu,
                async move |cpu, endpoint| {
                    let host = config.host.map(Clock::from_boxed_source);
                    Runner::with_guest_time(guest, host, cpu, endpoint).await
                },
                session,
            );
        }
        let (host, timers): (Box<dyn ClockSource + Send>, _) = match config.host {
            Some(host) => (host, None),
            None => {
                let (host, timers) = timer_service();
                (Box::new(host), Some(timers))
            }
        };
        let mut execution = Self::spawn_with(
            cpu,
            async move |cpu, endpoint| {
                Runner::new(Clock::from_boxed_source(host), cpu, endpoint).await
            },
            session,
        );
        execution.timers = timers.map(|driver| driver.spawn_with_peer(execution.wake_peer()));
        execution
    }

    fn spawn_with(
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
        Self {
            worker,
            timers: None,
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
