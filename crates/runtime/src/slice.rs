use crate::{
    backend::{Backend, EntryRequest},
    Result, Session,
};
use wasm86_machine::Environment;
use wasm86_x86::ExecutionExit;

/// Why a cooperative work slice returned with published state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliceStop {
    WorkLimit,
    HostRequested,
    Exited(ExecutionExit),
}

/// Work consumed and instructions retired before the slice returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SliceOutcome {
    /// Actual work consumed, including any overshoot beyond the allowance.
    pub work: u64,
    pub completed: u64,
    pub stop: SliceStop,
}

impl<B: Backend, D: Environment + 'static> Session<B, D> {
    /// Executes through Wasm dispatch until the work allowance is exhausted at a
    /// cooperative boundary. The allowance may be exceeded by at most 255 units.
    /// Only environment imports cross into the host before an exit or exhaustion.
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
            work: (i64::from(work_limit) - progress.entry.remaining_work) as u64,
            completed: progress.entry.completed,
            stop: progress
                .exit
                .map_or(SliceStop::WorkLimit, SliceStop::Exited),
        })
    }

    /// Executes with a cooperative work allowance: one completed instruction,
    /// including a whole REP, costs one unit.
    /// Execution can overshoot the allowance by at most 255 units. Faults retain
    /// precise state and completed work regardless of scheduling boundaries.
    ///
    /// The predicate observes the initial state and published dispatch boundaries.
    /// Compiled blocks need not expose each instruction. A predicate stop takes
    /// precedence over exhausted work; a guest exit returns immediately.
    /// A zero allowance does not fetch an instruction.
    ///
    /// Host callbacks remain synchronous. The allowance counts instructions, not
    /// transferred bytes or elapsed time. Browser callers should yield to their
    /// event loop between slices.
    pub fn run_slice_until(
        &mut self,
        work_limit: u32,
        mut should_stop: impl FnMut(&Self) -> bool,
    ) -> Result<SliceOutcome> {
        let mut remaining = i64::from(work_limit);
        let mut completed = 0;
        loop {
            let stop = if should_stop(self) {
                SliceStop::HostRequested
            } else if remaining <= 0 {
                SliceStop::WorkLimit
            } else {
                let progress = self.enter(EntryRequest::observed(remaining as u32))?;
                remaining = progress.entry.remaining_work;
                completed += progress.entry.completed;
                match progress.exit {
                    Some(exit) => SliceStop::Exited(exit),
                    None => continue,
                }
            };
            return Ok(SliceOutcome {
                work: (i64::from(work_limit) - remaining) as u64,
                completed,
                stop,
            });
        }
    }
}
