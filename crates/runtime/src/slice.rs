use crate::{
    backend::{Backend, EntryRequest},
    Devices, Machine, Result,
};
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
    /// Executes a work-bounded slice entirely through Wasm dispatch. Only device
    /// imports cross into the host before an exit or exhaustion.
    /// Use `run_slice_until` when a host predicate must observe dispatch boundaries.
    pub fn run_slice(&mut self, work_limit: u32) -> Result<SliceOutcome> {
        if work_limit == 0 {
            return Ok(SliceOutcome {
                work: 0,
                completed: 0,
                stop: SliceStop::WorkLimit,
            });
        }
        let progress = self.enter(EntryRequest::slice(work_limit))?;
        Ok(SliceOutcome {
            work: work_limit - progress.entry.remaining_work,
            completed: progress.entry.completed,
            stop: progress
                .exit
                .map_or(SliceStop::WorkLimit, SliceStop::Exited),
        })
    }

    /// Executes at most `work_limit` units of guest work: one ordinary instruction
    /// or one completed REP element. A zero-count REP costs one unit. Faulting
    /// operations do not consume work; earlier completed effects are retained.
    ///
    /// Unlike [`Self::run`], this can yield partway through REP with EIP unchanged
    /// and updated count/indices. The predicate observes the initial state and
    /// every published progress boundary, including partial REP chunks. It takes
    /// precedence over the work limit, but an exit returns
    /// immediately.
    /// Call again to resume; a zero limit does not fetch an instruction.
    ///
    /// Host callbacks remain synchronous. Work limits bound guest execution, not
    /// callback latency or elapsed time. Browser callers should yield to their
    /// event loop between slices.
    pub fn run_slice_until(
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
                let progress = self.enter(EntryRequest::observed(remaining))?;
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
