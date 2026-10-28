//! Cooperative CPU execution on one emulation owner.

use crate::{Clock, Cpu, MonotonicClock};
use relocade_x86::ExecutionExit;
use std::num::NonZeroU64;

/// Runs finite CPU quanta with a host event-loop opportunity between them.
/// Poll device futures alongside execution using ordinary async composition.
/// A synchronous CPU poll can exceed its instruction budget or a device deadline.
pub struct Runner<C = MonotonicClock> {
    clock: C,
    /// Finite budget requested for each CPU run. Checks remain at block boundaries.
    pub instruction_quantum: NonZeroU64,
}

impl Default for Runner {
    fn default() -> Self {
        Self::with_clock(MonotonicClock::default())
    }
}

impl<C: Clock> Runner<C> {
    pub fn with_clock(clock: C) -> Self {
        Self {
            clock,
            instruction_quantum: NonZeroU64::new(10_000).unwrap(),
        }
    }

    /// Shared elapsed timebase for device futures throughout the session.
    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// Returns the first non-budget exit, retaining each CPU run across awaits.
    /// The environment interprets the exit and decides when to run another process.
    pub async fn run<T: 'static>(&self, cpu: &mut Cpu<T>) -> ExecutionExit {
        loop {
            let exit = cpu.run(self.instruction_quantum.get()).await;
            if exit != ExecutionExit::BudgetExhausted {
                return exit;
            }
            self.clock.wait_until(self.clock.now()).await;
        }
    }
}
