//! Drives CPU turns, guest deadlines and host pacing.

mod slice;
use slice::ExecutionSlice;

use super::GuestTimeConfig;
use crate::{machine::execution::Member, Clock, Cpu, ExecutionInterrupt, GuestTimeDriver};
use relocade_x86::ExecutionExit;
use std::task::{ready, Context, Poll, Waker};

pub(super) enum Turn {
    Continue,
    /// An idle machine advanced without a host pacing wait.
    UnpacedIdle,
    Exit(ExecutionExit),
}

pub(super) struct ExecutionSchedule {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
    slice: Option<ExecutionSlice>,
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
            guest: Some(GuestSchedule { driver, slice }),
        }
    }

    pub(super) fn clock(&self) -> &Clock {
        &self.clock
    }

    pub(super) fn bind(&self, cpu: &Cpu, changed: &Waker) -> Option<impl Drop + use<>> {
        self.guest
            .as_ref()
            .map(|guest| guest.driver.bind(cpu, changed))
    }

    /// Polls host work before pacing guest entry, then publishes progress and
    /// chooses whether to yield, wait or return. Pending pacing waits do not
    /// advance idle time. A terminal exit includes CPU cleanup.
    pub(super) fn poll_turn(
        &mut self,
        mut cpu: Option<&mut Cpu>,
        execution: Option<&Member>,
        interrupt: &ExecutionInterrupt,
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
            cpu.map_or(Poll::Pending, |cpu| cpu.poll_execution(cx))
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
                // Let ready activities replace their waits before advancing idle
                // time. Neither a wake nor a replaced deadline resets the slice.
                if interrupt.is_requested() {
                    return Poll::Ready(Turn::Continue);
                }
                let Some(guest) = &self.guest else {
                    return Poll::Pending;
                };
                let Some(deadline) = guest.driver.next_deadline() else {
                    return Poll::Pending;
                };
                let (next, turn) = match &guest.slice {
                    Some(slice) if slice.paced => (deadline.min(slice.deadline()), Turn::Continue),
                    _ => (deadline, Turn::UnpacedIdle),
                };
                guest.driver.advance_to(next);
                Poll::Ready(turn)
            }
        }
    }
}
