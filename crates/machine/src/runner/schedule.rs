//! Drives CPU turns, guest deadlines and host pacing.

mod slice;
use slice::ExecutionSlice;

use super::GuestTimeConfig;
use crate::{machine::execution::Member, Clock, Cpu, GuestTimeDriver, Timestamp};
use relocade_x86::ExecutionExit;
use std::{
    rc::Rc,
    task::{ready, Context, Poll, Waker},
};

pub(super) enum Turn {
    Continue,
    Exit(ExecutionExit),
}

pub(super) struct ExecutionSchedule {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
    slice: Option<ExecutionSlice>,
    deferred_idle: Option<Timestamp>,
}

impl ExecutionSchedule {
    pub(super) fn from_clock(clock: Clock) -> Self {
        Self { clock, guest: None }
    }

    pub(super) fn with_guest_time(config: GuestTimeConfig) -> Self {
        let mut driver = GuestTimeDriver::new(config.instructions_per_second);
        let slice = config.slice_duration.map(|duration| {
            ExecutionSlice::new(&driver.clock(), duration, config.host_clock.is_some())
        });
        if let Some(host) = config.host_clock {
            driver = driver.with_host_clock(host);
        }
        Self {
            clock: driver.clock(),
            guest: Some(GuestSchedule {
                driver,
                slice,
                deferred_idle: None,
            }),
        }
    }

    pub(super) fn clock(&self) -> &Clock {
        &self.clock
    }

    #[must_use = "retain the exit and deadline wake bindings for the session"]
    pub(super) fn bind(
        &self,
        cpu: &Cpu,
        changed: &Waker,
    ) -> (Rc<Waker>, Option<impl Drop + use<>>) {
        let deadlines = self
            .guest
            .as_ref()
            .map(|guest| guest.driver.bind(cpu, changed));
        (cpu.exit_handle().bind(changed), deadlines)
    }

    /// Polls host work before pacing guest entry, then publishes progress and
    /// chooses whether to yield, wait or return. Pending pacing waits do not
    /// advance idle time. A terminal exit includes CPU cleanup.
    pub(super) fn poll_turn(
        &mut self,
        mut cpu: Option<&mut Cpu>,
        execution: Option<&Member>,
        has_ready: impl Fn() -> bool,
        cx: &mut Context<'_>,
    ) -> Poll<Turn> {
        let _execution = match execution {
            Some(execution) => Some(ready!(execution.poll_enter(cx))),
            None => None,
        };
        let result = if let Some(exit) = cpu.as_mut().and_then(|cpu| cpu.poll_host_work(cx)) {
            Poll::Ready(exit)
        } else {
            if let Some(guest) = &mut self.guest {
                if let Some(slice) = &mut guest.slice {
                    ready!(slice.poll(&self.clock, self.clock.now(), cx));
                }
                guest
                    .driver
                    .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
            }
            cpu.map_or(Poll::Pending, |cpu| {
                cpu.poll_scheduled_execution(cx, has_ready())
            })
        };
        if let Some(guest) = &self.guest {
            guest
                .driver
                .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
        }
        match result {
            Poll::Ready(ExecutionExit::Interrupted) => Poll::Ready(Turn::Continue),
            Poll::Ready(ExecutionExit::BudgetExhausted) if self.guest.is_some() => {
                Poll::Ready(Turn::Continue)
            }
            Poll::Ready(exit) => Poll::Ready(Turn::Exit(exit)),
            Poll::Pending => {
                let Some(guest) = &mut self.guest else {
                    return Poll::Pending;
                };
                let Some(deadline) = guest.driver.next_deadline() else {
                    guest.deferred_idle = None;
                    return Poll::Pending;
                };
                // Tasks already enqueued can cancel or replace the earliest wait.
                // One FIFO round is sufficient even when an activity wakes itself.
                if has_ready() && guest.deferred_idle != Some(deadline) {
                    guest.deferred_idle = Some(deadline);
                    return Poll::Ready(Turn::Continue);
                }
                guest.deferred_idle = None;
                let deadline = match &guest.slice {
                    Some(slice) if slice.paced => deadline.min(slice.deadline()),
                    _ => deadline,
                };
                guest.driver.advance_to(deadline);
                // Expiration wakes the owning task. The machine itself needs
                // another idle turn only if there is still a deadline to drive.
                if guest.driver.next_deadline().is_some() {
                    Poll::Ready(Turn::Continue)
                } else {
                    Poll::Pending
                }
            }
        }
    }
}
