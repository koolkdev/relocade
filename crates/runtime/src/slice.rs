use crate::{backend::Backend, Devices, Machine, Result};
use wasm86_x86::ExecutionExit;

/// Why a bounded work slice returned with published state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliceStop {
    WorkLimit,
    HostRequested,
    Exited(ExecutionExit),
}

/// Work and instruction retirement are independent: a slice can complete REP
/// elements without retiring the instruction. A fault preserves both counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SliceOutcome {
    pub work: u32,
    pub completed: u32,
    pub stop: SliceStop,
}

impl<B: Backend, D: Devices + 'static> Machine<B, D> {
    /// Executes at most `work_limit` units of guest work: one ordinary instruction
    /// or one completed REP element. A zero-count REP costs one unit. Faulting
    /// operations do not consume work; earlier completed effects are retained.
    ///
    /// Unlike [`Self::run`], this can yield partway through REP with EIP unchanged
    /// and updated count/indices. The predicate observes the initial state and
    /// every published progress boundary, including partial REP chunks. It takes
    /// precedence over the work limit, but a guest exit returns immediately.
    /// Call again to resume; a zero limit does not fetch an instruction.
    ///
    /// Host callbacks remain synchronous. Work limits bound guest execution, not
    /// callback latency or elapsed time. Browser callers should yield to their
    /// event loop between slices.
    pub fn run_slice(
        &mut self,
        work_limit: u32,
        mut should_stop: impl FnMut(&Self) -> bool,
    ) -> Result<SliceOutcome> {
        let mut remaining = work_limit;
        let mut completed = 0;
        loop {
            let stop = if should_stop(self) {
                SliceStop::HostRequested
            } else if remaining == 0 {
                SliceStop::WorkLimit
            } else {
                let progress = self.enter(remaining)?;
                remaining = progress.entry.remaining_work;
                completed += progress.entry.completed;
                match progress.exit {
                    Some(exit) => SliceStop::Exited(exit),
                    None => continue,
                }
            };
            return Ok(SliceOutcome {
                work: work_limit - remaining,
                completed,
                stop,
            });
        }
    }
}
