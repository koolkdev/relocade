//! Execution-local guest time and instruction targets.

#[cfg(test)]
mod tests;

mod wait;
use wait::{Source, Wait};

use super::{Clock, Instant};
use crate::{Cpu, CpuStateAccess, InstructionTarget};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker, time::Duration};

/// Owns emulated guest time, pending sleeps and the CPU instruction target.
/// Published retirement advances time at the configured instruction rate. The
/// embedding owns CPU execution and calls [`Self::synchronize`] at stopped
/// boundaries. It also decides when idle time may advance to the next deadline.
/// Devices use the common [`Clock`] returned by [`Self::clock`].
///
/// Callback observations can lag unpublished retirement; instruction targets
/// remain soft checkpoint limits. This driver does not pace against host time.
pub struct GuestTimeDriver(Rc<RefCell<State>>);

struct State {
    instructions_per_second: u64,
    now: Instant,
    remainder: u64,
    cpu: Option<Binding>,
    waits: Vec<Wait>,
}

struct Binding {
    state: CpuStateAccess,
    target: InstructionTarget,
    last_instruction_count: u64,
    yield_deadline: Option<Instant>,
    changed: Waker,
}

impl GuestTimeDriver {
    pub fn new(start: Instant, instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            instructions_per_second: instructions_per_second.get(),
            now: start,
            remainder: 0,
            cpu: None,
            waits: Vec::new(),
        })))
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
    /// embedding wants execution to yield. Wait changes notify the embedding;
    /// synchronize before resuming execution to account for them and after
    /// execution stops.
    pub fn synchronize(&self, yield_deadline: Option<Instant>) {
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

    fn next_deadline(&self) -> Option<Instant> {
        self.0.borrow().waits.iter().map(|wait| wait.deadline).min()
    }

    fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.observe();
            let due = state
                .waits
                .extract_if(.., |wait| wait.deadline <= now)
                .collect::<Vec<_>>();
            state.refresh_target();
            due
        };
        for wait in due {
            let _ = wait.complete.send(());
        }
    }

    fn advance_to(&self, deadline: Instant) {
        {
            let mut state = self.0.borrow_mut();
            state.now = state.observe().max(deadline);
        }
        self.notify_due();
    }
}

struct BindingGuard<'a>(&'a GuestTimeDriver);

impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0 .0.borrow_mut();
        state.observe();
        state.cpu.take().unwrap().target.set(None);
    }
}

impl State {
    fn observe(&mut self) -> Instant {
        if let Some(cpu) = &mut self.cpu {
            let instruction_count = cpu.state.instruction_count();
            let retired = instruction_count.wrapping_sub(cpu.last_instruction_count);
            let scaled = u128::from(retired) * 1_000_000_000 + u128::from(self.remainder);
            let nanos = scaled / u128::from(self.instructions_per_second);
            self.remainder = (scaled % u128::from(self.instructions_per_second)) as u64;
            self.now += Duration::new(
                (nanos / 1_000_000_000) as u64,
                (nanos % 1_000_000_000) as u32,
            );
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
            let remaining = deadline.duration_since(now).as_nanos();
            // Use the same published count and fractional progress for time and
            // target selection. Reprogramming cannot extend the yield deadline.
            let scaled = remaining
                .saturating_mul(u128::from(self.instructions_per_second))
                .saturating_sub(u128::from(self.remainder));
            let allowance = scaled.div_ceil(1_000_000_000).min(i64::MAX as u128) as u64;
            cpu.last_instruction_count.wrapping_add(allowance)
        });
        cpu.target.set(target);
    }
}
