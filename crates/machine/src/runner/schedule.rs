//! Drives CPU turns, guest deadlines and host pacing.

mod slice;
use slice::ExecutionSlice;

use super::GuestTimeConfig;
use crate::{Clock, Cpu, GuestTimeDriver, Instant};
use relocade_x86::ExecutionExit;
use std::task::{ready, Context, Poll, Waker};

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
        let mut driver = GuestTimeDriver::new(Instant::now(), config.instructions_per_second);
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

    /// Retains the slice boundary across runs while cancelling each run's host wait.
    #[must_use]
    pub(super) fn bind<'a>(&'a self, cpu: &Cpu, changed: &Waker) -> Option<impl Drop + use<'a>> {
        self.guest.as_ref().map(|guest| Binding {
            _cpu: guest.driver.bind(cpu, changed),
            slice: guest.slice.as_ref(),
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
                if let Some(slice) = &guest.slice {
                    ready!(slice.poll(&self.clock, self.clock.now(), cx));
                }
                guest
                    .driver
                    .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
            }
            cpu.poll_execution(cx)
        };
        if let Some(guest) = &self.guest {
            guest
                .driver
                .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
        }
        match result {
            Poll::Ready(ExecutionExit::Interrupted) => Poll::Ready(None),
            Poll::Ready(ExecutionExit::BudgetExhausted) if self.guest.is_some() => {
                Poll::Ready(None)
            }
            Poll::Ready(exit) => Poll::Ready(Some(exit)),
            Poll::Pending => {
                // Let ready activities replace their waits before advancing idle
                // time. Neither a wake nor a replaced deadline resets the slice.
                if cpu.interrupt_handle().is_requested() {
                    return Poll::Ready(None);
                }
                let Some(guest) = &self.guest else {
                    return Poll::Pending;
                };
                let Some(mut deadline) = guest.driver.next_deadline() else {
                    return Poll::Pending;
                };
                if let Some(slice) = &guest.slice {
                    deadline = slice.idle_deadline(deadline);
                }
                guest.driver.advance_to(deadline);
                Poll::Ready(None)
            }
        }
    }
}

struct Binding<'a, T> {
    _cpu: T,
    slice: Option<&'a ExecutionSlice>,
}

impl<T> Drop for Binding<'_, T> {
    fn drop(&mut self) {
        if let Some(slice) = self.slice {
            slice.cancel_wait();
        }
    }
}
