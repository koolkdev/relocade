//! Drives CPU turns, guest deadlines and host pacing.

mod slice;
use slice::ExecutionSlice;

use super::GuestTimeConfig;
use crate::{Clock, Cpu, GuestTimeDriver};
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
        let _execution = ready!(cpu.poll_enter_execution(cx));
        let result = if let Some(exit) = cpu.poll_host_work(cx) {
            Poll::Ready(exit)
        } else {
            ready!(self.poll_slice(cx));
            self.synchronize();
            cpu.poll_execution(cx)
        };
        self.synchronize();
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
                self.advance_idle().map(|()| None)
            }
        }
    }

    /// A native activity owns the CPU. Its devices can reach their next deadline
    /// under the same machine pause and slice limits as a halted guest.
    pub(super) fn poll_idle(&self, cpu: &Cpu, cx: &mut Context<'_>) -> Poll<()> {
        let _execution = ready!(cpu.poll_enter_execution(cx));
        ready!(self.poll_slice(cx));
        self.advance_idle()
    }

    fn poll_slice(&self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(slice) = self.guest.as_ref().and_then(|guest| guest.slice.as_ref()) {
            return slice.poll(&self.clock, self.clock.now(), cx);
        }
        Poll::Ready(())
    }

    fn synchronize(&self) {
        if let Some(guest) = &self.guest {
            guest
                .driver
                .synchronize(guest.slice.as_ref().map(ExecutionSlice::deadline));
        }
    }

    fn advance_idle(&self) -> Poll<()> {
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
        Poll::Ready(())
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
