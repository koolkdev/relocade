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
    // One retired instruction contributes 1e9 ticks; one nanosecond contributes
    // the instruction rate. Reading progress requires no time conversion.
    now: Timestamp,
    cpu: Option<Binding>,
    waits: Vec<Wait>,
    host: Option<HostTime>,
}

struct Binding {
    state: CpuStateAccess,
    target: InstructionTarget,
    last_instruction_count: u64,
    yield_deadline: Option<Timestamp>,
    changed: Waker,
}

impl GuestTimeDriver {
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            timebase: Timebase(instructions_per_second),
            now: Timestamp(0),
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
        let now = state.observe();
        if let Some(host) = &mut state.host {
            host.resume(now);
        }
        let cpu_state = cpu.cpu_state();
        state.cpu = Some(Binding {
            last_instruction_count: cpu_state.instruction_count(),
            state: cpu_state,
            target: cpu.instruction_target(),
            yield_deadline: None,
            changed: changed.clone(),
        });
        state.refresh_target();
        BindingGuard(self)
    }

    /// Updates guest time from retirement, completes due sleeps and arms the CPU
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
        self.0.borrow().waits.iter().map(|wait| wait.deadline).min()
    }

    fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.observe();
            let lag = state.host.as_mut().and_then(|host| host.discard_lag(now));
            let timebase = state.timebase;
            let due = state
                .waits
                .extract_if(.., |wait| {
                    if let Some(lag) = lag.filter(|_| wait.host_sync) {
                        wait.deadline = timebase.checked_sub(wait.deadline, lag).unwrap_or(now);
                    }
                    wait.deadline <= now
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
            let now = state.observe();
            state.now = now.max(deadline);
        }
        self.notify_due();
    }
}

struct BindingGuard<'a>(&'a GuestTimeDriver);

impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0 .0.borrow_mut();
        state.observe();
        if let Some(host) = &mut state.host {
            host.pause();
        }
        state.cpu.take().unwrap().target.set(None);
    }
}

impl State {
    fn observe(&mut self) -> Timestamp {
        if let Some(cpu) = &mut self.cpu {
            let instruction_count = cpu.state.instruction_count();
            let retired = instruction_count.wrapping_sub(cpu.last_instruction_count);
            let progress = u128::from(retired) * 1_000_000_000;
            self.now.0 = self
                .now
                .0
                .checked_add(progress)
                .expect("guest clock exhausted");
            cpu.last_instruction_count = instruction_count;
        }
        self.now
    }

    fn refresh_target(&mut self) {
        let now = self.observe();
        let Some(cpu) = &self.cpu else { return };
        let deadline = self
            .waits
            .iter()
            .map(|wait| wait.deadline)
            .chain(cpu.yield_deadline)
            .min();
        let target = deadline.map(|deadline| {
            // Round up in native units so execution never stops before a deadline.
            let remaining = deadline.0.saturating_sub(now.0);
            let allowance = remaining.div_ceil(1_000_000_000).min(i64::MAX as u128) as u64;
            cpu.last_instruction_count.wrapping_add(allowance)
        });
        cpu.target.set(target);
    }
}
