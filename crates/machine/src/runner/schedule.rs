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

pub(super) struct ExecutionSchedule {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
    slice: Option<ExecutionSlice>,
}

pub(super) enum MachineTurn<'a> {
    Checkpoint,
    Exit(ExecutionExit),
    IdleUntil(&'a GuestTimeDriver, Timestamp),
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

    /// Runs to a CPU checkpoint or exit. A blocked CPU offers its next deadline
    /// to the runner, which knows whether another Rust activity is ready.
    pub(super) fn poll_turn(
        &mut self,
        mut cpu: Option<&mut Cpu>,
        execution: Option<&Member>,
        cx: &mut Context<'_>,
    ) -> Poll<MachineTurn<'_>> {
        let _execution = match execution {
            Some(execution) => Some(ready!(execution.poll_enter(cx))),
            None => None,
        };
        // A host service can complete even while guest entry is paced.
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
            cpu.map_or(Poll::Pending, |cpu| cpu.poll_execution(cx, true))
        };
        let exit = match result {
            Poll::Ready(exit) => exit,
            Poll::Pending => {
                let Some(guest) = &self.guest else {
                    return Poll::Pending;
                };
                let Some(deadline) = guest.driver.next_deadline() else {
                    return Poll::Pending;
                };
                let deadline = match &guest.slice {
                    Some(slice) if slice.paced => deadline.min(slice.deadline()),
                    _ => deadline,
                };
                return Poll::Ready(MachineTurn::IdleUntil(&guest.driver, deadline));
            }
        };
        if let Some(guest) = &self.guest {
            guest.driver.notify_due();
        }
        match exit {
            ExecutionExit::Interrupted => Poll::Ready(MachineTurn::Checkpoint),
            ExecutionExit::BudgetExhausted if self.guest.is_some() => {
                Poll::Ready(MachineTurn::Checkpoint)
            }
            exit => Poll::Ready(MachineTurn::Exit(exit)),
        }
    }
}
