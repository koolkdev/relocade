//! Execution-local guest time and instruction targets.

#[cfg(test)]
mod tests;

mod host;
use host::HostTime;

mod wait;
use wait::{Source, Wait};

use super::{Clock, Timebase, Timestamp};
use crate::{Cpu, CpuStateAccess, InstructionTarget};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker};

/// Owns emulated guest time, pending sleeps and the CPU instruction target.
/// Published retirement advances time at the configured instruction rate. The
/// embedding owns CPU execution and calls [`Self::synchronize`] at stopped
/// boundaries. It also decides when idle time may advance to the next deadline.
/// Devices use the common [`Clock`] returned by [`Self::clock`].
///
/// Callback observations can lag unpublished retirement; instruction targets
/// remain soft checkpoint limits. Runner chooses pacing boundaries; the driver
/// retains the optional host/guest time correspondence.
pub struct GuestTimeDriver(Rc<RefCell<State>>);

struct State {
    timebase: Timebase,
    // While bound, guest ticks are the published instruction count plus this
    // offset. While paused, the offset holds the frozen reading.
    offset: u64,
    cpu: Option<Binding>,
    waits: Vec<Wait>,
    host: Option<HostTime>,
}

struct Binding {
    state: CpuStateAccess,
    target: InstructionTarget,
    yield_deadline: Option<Timestamp>,
    changed: Waker,
}

impl GuestTimeDriver {
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            timebase: Timebase(instructions_per_second),
            offset: 0,
            cpu: None,
            waits: Vec::new(),
            host: None,
        })))
    }

    pub(crate) fn with_host_clock(self, clock: Clock) -> Self {
        let mut state = self.0.borrow_mut();
        state.host = Some(HostTime::new(clock, state.timebase));
        drop(state);
        self
    }

    pub fn clock(&self) -> Clock {
        Clock::new(Source(self.0.clone()))
    }

    /// Observes CPU retirement and controls its target until the returned guard
    /// is dropped. Preexisting retirement is not charged to the clock. Dropping
    /// the guard records final progress, clears the target and releases the waker.
    /// The clock pauses between bindings; pending device waits remain registered.
    ///
    /// `changed` wakes the embedding when waits change, including while a CPU
    /// service is pending. The embedding must be this CPU's only target writer.
    /// Panics if the driver already has a live binding.
    #[must_use = "retain the binding while driving this CPU"]
    pub fn bind<'a>(&'a self, cpu: &Cpu, changed: &Waker) -> impl Drop + use<'a> {
        let mut state = self.0.borrow_mut();
        assert!(state.cpu.is_none(), "guest time driver is already bound");
        let now = state.now();
        if let Some(host) = &mut state.host {
            host.resume(now);
        }
        let cpu_state = cpu.cpu_state();
        state.offset = now.ticks().wrapping_sub(cpu_state.instruction_count());
        state.cpu = Some(Binding {
            state: cpu_state,
            target: cpu.instruction_target(),
            yield_deadline: None,
            changed: changed.clone(),
        });
        state.refresh_target();
        BindingGuard(self)
    }

    /// Reads guest time, completes due sleeps and arms the CPU
    /// target. `yield_deadline` is an optional absolute guest time at which the
    /// embedding wants execution to yield. Wait changes preserve it until the next
    /// synchronization. With host pacing, lag over 20 ms resets the correspondence
    /// and shortens pending external sync waits without moving guest time. Call
    /// after execution stops and after ready activities have had a turn.
    pub fn synchronize(&self, yield_deadline: Option<Timestamp>) {
        if let Some(cpu) = &mut self.0.borrow_mut().cpu {
            cpu.yield_deadline = yield_deadline;
        }
        self.notify_due();
    }

    /// Advances guest time to the earliest registered sleep and completes due sleeps.
    /// Returns false when no sleep exists. Call only after ready activities have
    /// been polled and execution is still idle. The yield deadline does not cause
    /// this operation to advance time.
    pub fn advance_to_next_deadline(&self) -> bool {
        let Some(deadline) = self.next_deadline() else {
            return false;
        };
        self.advance_to(deadline);
        true
    }

    /// Earliest registered sleep in guest time. Query at a stopped boundary
    /// after ready activities have had an opportunity to replace their waits.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.0
            .borrow()
            .waits
            .iter()
            .map(|wait| wait.deadline.deadline())
            .min()
    }

    fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            let lag = state.host.as_mut().and_then(|host| host.discard_lag(now));
            let timebase = state.timebase;
            let due = state
                .waits
                .extract_if(.., |wait| {
                    if let Some(lag) = lag.filter(|_| wait.host_sync) {
                        wait.deadline = timebase
                            .checked_retreat(wait.deadline, lag)
                            .unwrap_or_else(|| now.into());
                    }
                    wait.deadline.deadline() <= now
                })
                .collect::<Vec<_>>();
            state.refresh_target();
            due
        };
        for wait in due {
            let _ = wait.complete.send(());
        }
    }

    /// Advance only after execution and ready activities have become idle.
    pub(crate) fn advance_to(&self, deadline: Timestamp) {
        {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            if deadline > now {
                state.offset = state
                    .offset
                    .wrapping_add(deadline.ticks().wrapping_sub(now.ticks()));
            }
        }
        self.notify_due();
    }
}

struct BindingGuard<'a>(&'a GuestTimeDriver);

impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0 .0.borrow_mut();
        let now = state.now();
        if let Some(host) = &mut state.host {
            host.pause();
        }
        state.cpu.take().unwrap().target.set(None);
        state.offset = now.ticks();
    }
}

impl State {
    fn now(&self) -> Timestamp {
        let count = self
            .cpu
            .as_ref()
            .map_or(0, |cpu| cpu.state.instruction_count());
        Timestamp::from_ticks(count.wrapping_add(self.offset))
    }

    fn refresh_target(&mut self) {
        let now = self.now();
        let Some(cpu) = &self.cpu else { return };
        let deadline = self
            .waits
            .iter()
            .map(|wait| wait.deadline.deadline())
            .chain(cpu.yield_deadline)
            .min();
        let target = deadline.map(|deadline| deadline.max(now).ticks().wrapping_sub(self.offset));
        cpu.target.set(target);
    }
}
