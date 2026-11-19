//! Execution-local guest time and instruction targets.

#[cfg(test)]
mod tests;

mod wait;
use wait::{Source, Wait};

use super::{Clock, TickRate, Timestamp};
use crate::{Cpu, CpuStateAccess, InstructionTarget};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker};

/// Owns emulated guest time, pending sleeps and the CPU instruction target.
/// Published retirement advances time at the configured instruction rate. The
/// embedding owns CPU execution and calls [`Self::synchronize`] at stopped
/// boundaries. It also decides when idle time may advance to the next deadline.
/// Devices use the common [`Clock`] returned by [`Self::clock`].
///
/// Callback observations can lag unpublished retirement; instruction targets
/// remain soft checkpoint limits. Profiles compiled without budget checks return
/// via an execution interrupt at each completed block while a deadline is armed.
/// The runner chooses scheduling boundaries.
/// Output can wait for remaining guest intervals on an attached host clock.
pub struct GuestTimeDriver(Rc<RefCell<State>>);

struct State {
    tick_rate: TickRate,
    // While bound, guest ticks are the published instruction count plus this
    // offset. While paused, the offset holds the frozen reading.
    offset: u64,
    cpu: Option<Binding>,
    waits: Vec<Rc<Wait>>,
    host: Option<Clock>,
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
            tick_rate: TickRate::new(instructions_per_second),
            offset: 0,
            cpu: None,
            waits: Vec::new(),
            host: None,
        })))
    }

    pub(crate) fn with_host_clock(self, clock: Clock) -> Self {
        self.0.borrow_mut().host = Some(clock);
        self
    }

    pub fn clock(&self) -> Clock {
        Clock::new(Source(self.0.clone()))
    }

    /// Observes CPU retirement and controls its target until the returned guard
    /// is dropped. Preexisting retirement is not charged to the clock. Dropping
    /// the guard records final progress, clears the clock's deadline and releases the waker.
    /// The clock pauses between bindings; pending device waits remain registered.
    ///
    /// `changed` wakes the embedding when waits change, including while a CPU
    /// service is pending. The clock's deadlines coexist with caller instruction limits.
    /// Panics if the driver already has a live binding.
    #[must_use = "retain the binding while driving this CPU"]
    pub fn bind(&self, cpu: &Cpu, changed: &Waker) -> impl Drop + use<> {
        let mut state = self.0.borrow_mut();
        assert!(state.cpu.is_none(), "guest time driver is already bound");
        let now = state.now();
        let cpu_state = cpu.cpu_state();
        state.offset = now.ticks().wrapping_sub(cpu_state.instruction_count());
        state.cpu = Some(Binding {
            state: cpu_state,
            target: cpu.instruction_target(),
            yield_deadline: None,
            changed: changed.clone(),
        });
        state.refresh_target();
        BindingGuard(self.0.clone())
    }

    /// Reads guest time, completes due sleeps and arms the CPU
    /// target. `yield_deadline` is an optional absolute guest time at which the
    /// embedding wants execution to yield. Wait changes preserve it until the next
    /// synchronization. Call after execution stops and ready activities have had a turn.
    pub fn synchronize(&self, yield_deadline: Option<Timestamp>) {
        if let Some(cpu) = &mut self.0.borrow_mut().cpu {
            cpu.yield_deadline = yield_deadline;
        }
        self.notify_due();
    }

    /// The machine completes due device sleeps after retirement. Before guest
    /// entry it only needs to publish a changed slice or throughput checkpoint.
    pub(crate) fn set_yield_deadline(&self, deadline: Option<Timestamp>) {
        let mut state = self.0.borrow_mut();
        if let Some(cpu) = &mut state.cpu {
            // Profiles without budget checks use a block-end interrupt. The
            // worker may have acknowledged it since the previous CPU turn.
            if cpu.yield_deadline != deadline || !cpu.state.execution_target().instruction_budgets {
                cpu.yield_deadline = deadline;
                state.refresh_target();
            }
        }
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

    pub(crate) fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            let due = state
                .waits
                .extract_if(.., |wait| wait.deadline <= now)
                .map(|wait| wait.waker.take().expect("registered guest sleep"))
                .collect::<Vec<_>>();
            state.refresh_target();
            due
        };
        for waker in due {
            waker.wake();
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

struct BindingGuard(Rc<RefCell<State>>);

impl Drop for BindingGuard {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        let now = state.now();
        state.cpu.take().unwrap().target.set_deadline(None);
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
            .map(|wait| wait.deadline)
            .chain(cpu.yield_deadline)
            .min();
        if cpu.state.execution_target().instruction_budgets {
            let target =
                deadline.map(|deadline| deadline.max(now).ticks().wrapping_sub(self.offset));
            cpu.target.set_deadline(target);
        } else if deadline.is_some() {
            // These compiled entries check only the interrupt at block end.
            // Re-evaluate retirement at that boundary until the wait is due.
            cpu.state.interrupt_handle().request();
        }
    }
}
