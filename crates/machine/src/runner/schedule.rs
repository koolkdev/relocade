//! Drives CPU turns, guest deadlines and host pacing.

mod pacing;
use pacing::{next_boundary, Pacing};

use super::GuestTimeConfig;
use crate::{Clock, Cpu, GuestTimeDriver, Instant};
use relocade_x86::ExecutionExit;
use std::{
    cell::Cell,
    task::{ready, Context, Poll, Waker},
    time::Duration,
};

pub(super) struct ExecutionSchedule {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
    yield_origin: Instant,
    yield_interval: Option<Duration>,
    execution_deadline: Cell<Option<Instant>>,
    pacing: Option<Pacing>,
}

impl ExecutionSchedule {
    pub(super) fn from_clock(clock: Clock) -> Self {
        Self { clock, guest: None }
    }

    pub(super) fn with_guest_time(config: GuestTimeConfig) -> Self {
        let start = Instant::now();
        let mut driver = GuestTimeDriver::new(start, config.instructions_per_second);
        let pacing = if let Some(host) = config.host_clock {
            driver = driver.with_host_clock(host);
            Some(Pacing::new(config.burst_interval))
        } else {
            None
        };
        Self {
            clock: driver.clock(),
            guest: Some(GuestSchedule {
                driver,
                yield_origin: start,
                yield_interval: config.yield_interval,
                execution_deadline: Cell::new(None),
                pacing,
            }),
        }
    }

    pub(super) fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Retains pacing credit across runs while cancelling each run's host wait.
    #[must_use]
    pub(super) fn bind<'a>(&'a self, cpu: &Cpu, changed: &Waker) -> Option<impl Drop + use<'a>> {
        self.guest.as_ref().map(|guest| {
            let cpu = guest.driver.bind(cpu, changed);
            if let Some(pacing) = &guest.pacing {
                pacing.begin(self.clock.now());
            }
            Binding {
                _cpu: cpu,
                pacing: guest.pacing.as_ref(),
            }
        })
    }

    /// Polls host work before pacing guest entry, then publishes progress and
    /// chooses whether to yield, wait or return. Pending pacing waits do not
    /// advance idle time. Ready(None) gives activities a turn; Ready(Some(exit))
    /// returns the CPU result, with terminal-exit cleanup already complete.
    pub(super) fn poll_cpu(
        &self,
        cpu: &mut Cpu,
        cx: &mut Context<'_>,
    ) -> Poll<Option<ExecutionExit>> {
        let result = if let Some(exit) = cpu.poll_host_work(cx) {
            Poll::Ready(exit)
        } else {
            if let Some(guest) = &self.guest {
                let now = self.clock.now();
                if let Some(pacing) = &guest.pacing {
                    ready!(pacing.poll(&guest.driver, now, cx));
                }
                let deadline = guest
                    .yield_interval
                    .map(|interval| next_boundary(now, guest.yield_origin, interval))
                    .into_iter()
                    .chain(guest.pacing.as_ref().map(Pacing::deadline))
                    .min();
                guest.execution_deadline.set(deadline);
                guest.driver.synchronize(deadline);
            }
            cpu.poll_execution(cx)
        };
        if let Some(guest) = &self.guest {
            guest.driver.synchronize(guest.execution_deadline.get());
        }
        match result {
            Poll::Ready(ExecutionExit::Interrupted) => Poll::Ready(None),
            Poll::Ready(ExecutionExit::BudgetExhausted) if self.guest.is_some() => {
                Poll::Ready(None)
            }
            Poll::Ready(exit) => Poll::Ready(Some(exit)),
            Poll::Pending => {
                // Let ready activities replace their waits before advancing idle
                // time. Neither a wake nor a replaced deadline grants a new burst.
                if cpu.interrupt_handle().is_requested() {
                    return Poll::Ready(None);
                }
                let Some(guest) = &self.guest else {
                    return Poll::Pending;
                };
                let Some(mut deadline) = guest.driver.next_deadline() else {
                    return Poll::Pending;
                };
                if let Some(pacing) = &guest.pacing {
                    deadline = deadline.min(pacing.deadline());
                }
                guest.driver.advance_to(deadline);
                Poll::Ready(None)
            }
        }
    }
}

struct Binding<'a, T> {
    _cpu: T,
    pacing: Option<&'a Pacing>,
}

impl<T> Drop for Binding<'_, T> {
    fn drop(&mut self) {
        if let Some(pacing) = self.pacing {
            pacing.cancel_wait();
        }
    }
}
