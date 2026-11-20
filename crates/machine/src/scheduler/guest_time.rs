//! Execution-local guest time and device deadlines.

#[cfg(test)]
mod tests;

mod interpolation;
mod realtime;
mod session;
mod wait;

use realtime::Realtime;
pub(super) use session::BindingGuard;
use wait::{Source, Waits};

use crate::{Clock, ConfigurationError, Cpu, CpuStateAccess, TickRate, Timestamp};
use relocade_x86::ExecutionTarget;
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker};

/// Owns emulated guest time, pending sleeps and the CPU instruction target.
/// Published retirement advances time at the configured instruction rate. The
/// embedding owns CPU execution and calls [`Self::synchronize`] at stopped
/// boundaries. It also decides when idle time may advance to the next deadline.
/// Devices use the common [`Clock`] returned by [`Self::clock`].
///
/// CPU checkpoints occur at completed blocks. Live PC sessions calibrate
/// retirement against their host clock between blocks.
pub struct GuestTimeDriver(Rc<RefCell<State>>);

struct State {
    tick_rate: TickRate,
    // The published guest timestamp, or the retirement offset of a bound
    // deterministic clock.
    offset: u64,
    cpu: Option<Binding>,
    waits: Waits,
    realtime: Option<Realtime>,
}

struct Binding {
    state: CpuStateAccess,
    pacing_target: Option<u64>,
    changed: Waker,
}

impl GuestTimeDriver {
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            tick_rate: TickRate::new(instructions_per_second),
            offset: 0,
            cpu: None,
            waits: Waits::default(),
            realtime: None,
        })))
    }

    pub(crate) fn with_host_clock(self, clock: Clock) -> Self {
        let rate = self.0.borrow().tick_rate;
        self.0.borrow_mut().realtime = Some(Realtime::new(clock, rate));
        self
    }

    pub(crate) fn validate_target(target: ExecutionTarget) -> Result<(), ConfigurationError> {
        if target.instruction_budgets {
            Ok(())
        } else {
            Err(ConfigurationError::GuestTimeRequiresInstructionBudgets)
        }
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
    /// service is pending.
    /// Panics if the driver is already bound or the CPU disables instruction budgets.
    #[must_use = "retain the binding while driving this CPU"]
    pub fn bind(&self, cpu: &Cpu, changed: &Waker) -> impl Drop + use<> {
        Self::validate_target(cpu.cpu_state().execution_target()).unwrap();
        self.start_session(cpu, changed)
    }

    pub(super) fn start_session(&self, cpu: &Cpu, changed: &Waker) -> BindingGuard {
        let mut state = self.0.borrow_mut();
        assert!(state.cpu.is_none(), "guest time driver is already bound");
        let now = state.now();
        let cpu_state = cpu.cpu_state();
        if let Some(realtime) = &mut state.realtime {
            realtime.start(now, cpu_state.instruction_count());
        } else {
            state.offset = now.ticks().wrapping_sub(cpu_state.instruction_count());
        }
        state.cpu = Some(Binding {
            state: cpu_state,
            pacing_target: None,
            changed: changed.clone(),
        });
        state.refresh_target();
        BindingGuard {
            driver: Self(self.0.clone()),
        }
    }

    pub(super) fn set_pacing_target(&self, count: u64) {
        let mut state = self.0.borrow_mut();
        let cpu = state.cpu.as_mut().expect("pacing a bound guest CPU");
        if cpu.pacing_target.replace(count) != Some(count) {
            state.refresh_target();
        }
    }

    /// Advances guest time to the earliest registered sleep and completes due sleeps.
    /// Returns false when no sleep exists. Call only after ready tasks have
    /// been polled and execution is still idle.
    pub fn advance_to_next_deadline(&self) -> bool {
        let Some(deadline) = self.next_deadline() else {
            return false;
        };
        self.advance_to(deadline);
        true
    }

    /// Earliest registered sleep in guest time. Query at a stopped boundary
    /// after ready tasks have had an opportunity to replace their waits.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.0.borrow().waits.earliest
    }

    /// Wakes due guest sleeps and updates the CPU checkpoint. Returns whether any sleep was due.
    pub fn synchronize(&self) -> bool {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            let due = state.waits.take_due(now);
            if !due.is_empty() {
                state.refresh_target();
            }
            due
        };
        let woke_tasks = !due.is_empty();
        for waker in due {
            waker.wake();
        }
        woke_tasks
    }

    /// Advance only after execution and ready tasks have become idle.
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
        self.synchronize();
    }
}

impl State {
    fn live_clock(&mut self) -> Option<(&mut Realtime, u64)> {
        Some((
            self.realtime.as_mut()?,
            self.cpu.as_ref()?.state.instruction_count(),
        ))
    }

    fn now(&self) -> Timestamp {
        let Some(cpu) = &self.cpu else {
            return Timestamp::from_ticks(self.offset);
        };
        let count = cpu.state.instruction_count();
        match &self.realtime {
            Some(live) => live.estimate.now(count),
            None => Timestamp::from_ticks(count.wrapping_add(self.offset)),
        }
    }

    fn refresh_target(&self) {
        let Some(cpu) = &self.cpu else { return };
        let deadline = self.waits.earliest;
        let count = cpu.state.instruction_count();
        let target = match &self.realtime {
            // A resumed CPU returns after its first block to arm the host sample.
            Some(live) if !live.running => Some(count.wrapping_add(1)),
            Some(live) => deadline.and_then(|at| live.estimate.target(at, count)),
            None => deadline.map(|at| {
                let now = Timestamp::from_ticks(count.wrapping_add(self.offset));
                at.max(now).ticks().wrapping_sub(self.offset)
            }),
        };
        let next = target
            .into_iter()
            .chain(cpu.pacing_target)
            .min_by_key(|at| at.wrapping_sub(count) as i64);
        cpu.state.set_instruction_target(next);
    }
}
