//! Execution deadlines and idle scheduling, independent of CPU polling.

use crate::{Clock, Cpu, ExecutionInterrupt, GuestTimeDriver, Instant};
use relocade_x86::ExecutionExit;
use std::{
    cell::Cell,
    num::NonZeroU64,
    task::{Poll, Waker},
    time::Duration,
};

pub(super) struct ExecutionSchedule {
    clock: Clock,
    guest_time: Option<GuestTimeDriver>,
    yield_origin: Instant,
    yield_interval: Option<Duration>,
    yield_deadline: Cell<Option<Instant>>,
}

impl ExecutionSchedule {
    pub(super) fn from_clock(clock: Clock) -> Self {
        Self {
            yield_origin: clock.now(),
            clock,
            guest_time: None,
            yield_interval: None,
            yield_deadline: Cell::new(None),
        }
    }

    pub(super) fn with_guest_time(
        instructions_per_second: NonZeroU64,
        yield_interval: Option<Duration>,
    ) -> Self {
        assert!(
            yield_interval.is_none_or(|interval| !interval.is_zero()),
            "guest time scheduling needs a positive yield interval"
        );
        let start = Instant::now();
        let driver = GuestTimeDriver::new(start, instructions_per_second);
        Self {
            clock: driver.clock(),
            guest_time: Some(driver),
            yield_origin: start,
            yield_interval,
            yield_deadline: Cell::new(None),
        }
    }

    pub(super) fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Keeps target ownership and wait-change notifications scoped to one run.
    #[must_use]
    pub(super) fn bind<'a>(&'a self, cpu: &Cpu, changed: &Waker) -> Option<impl Drop + use<'a>> {
        self.guest_time
            .as_ref()
            .map(|driver| driver.bind(cpu, changed))
    }

    pub(super) fn before_cpu_poll(&self) {
        let Some(driver) = &self.guest_time else {
            return;
        };
        let deadline = self.yield_interval.map(|interval| {
            let now = self.clock.now();
            let remaining = interval.as_nanos()
                - now.duration_since(self.yield_origin).as_nanos() % interval.as_nanos();
            now + Duration::new(
                (remaining / 1_000_000_000) as u64,
                (remaining % 1_000_000_000) as u32,
            )
        });
        self.yield_deadline.set(deadline);
        driver.synchronize(deadline);
    }

    /// Publishes retirement before devices run, including on terminal exits.
    /// Ready(None) requests an activity turn; Ready(Some(exit)) ends the run
    /// after that turn. Pending parks until an external notification arrives.
    pub(super) fn after_cpu_poll(
        &self,
        result: Poll<ExecutionExit>,
        interrupt: &ExecutionInterrupt,
    ) -> Poll<Option<ExecutionExit>> {
        if let Some(driver) = &self.guest_time {
            driver.synchronize(self.yield_deadline.get());
        }
        match result {
            Poll::Ready(ExecutionExit::Interrupted) => Poll::Ready(None),
            Poll::Ready(ExecutionExit::BudgetExhausted) if self.guest_time.is_some() => {
                Poll::Ready(None)
            }
            Poll::Ready(exit) => Poll::Ready(Some(exit)),
            Poll::Pending => {
                // Ready activities may replace a wait. Give them a turn before
                // choosing an idle deadline, then poll the CPU again.
                if interrupt.is_requested()
                    || self
                        .guest_time
                        .as_ref()
                        .is_some_and(|driver| driver.advance_to_next_deadline())
                {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        }
    }
}
