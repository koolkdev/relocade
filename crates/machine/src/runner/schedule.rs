//! Guest deadlines and host pacing for the machine turn.

mod slice;
use slice::ExecutionSlice;

use super::GuestTimeConfig;
use crate::{Clock, Cpu, GuestTimeDriver};
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

    /// Waits for the host pace boundary and arms the next guest checkpoint.
    pub(super) fn poll_pacing(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(guest) = &mut self.guest {
            if let Some(slice) = &mut guest.slice {
                ready!(slice.poll(&self.clock, self.clock.now(), cx));
            }
            guest
                .driver
                .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
        }
        Poll::Ready(())
    }

    /// Publishes retirements and wakes any due waits after a CPU turn.
    pub(super) fn notify_due(&self) {
        if let Some(guest) = &self.guest {
            guest.driver.notify_due();
        }
    }

    /// Advances a blocked machine to its earliest guest deadline, within its pacing allowance.
    pub(super) fn advance_idle(&self) -> bool {
        let Some(guest) = &self.guest else {
            return false;
        };
        let Some(mut deadline) = guest.driver.next_deadline() else {
            return false;
        };
        if let Some(slice) = &guest.slice {
            if slice.paced {
                deadline = deadline.min(slice.deadline());
            }
        }
        guest.driver.advance_to(deadline);
        true
    }
}
