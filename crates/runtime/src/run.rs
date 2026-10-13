use crate::{backend::Backend, Devices, Machine, Result, StepOutcome};
use wasm86_x86::ExecutionExit;

/// Why a run returned with published guest state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunStop {
    InstructionLimit,
    HostRequested,
    Exited(ExecutionExit),
}

/// Progress within one call to [`Machine::run`]. A faulting or unsupported
/// instruction is not completed, even if it retains partial memory/device effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    pub completed: u64,
    pub stop: RunStop,
}

impl<B: Backend, D: Devices + 'static> Machine<B, D> {
    /// Executes at most `instruction_limit` complete x86 instructions.
    ///
    /// `should_stop` observes the initial state and every successful instruction
    /// boundary before another instruction is attempted. It takes precedence over
    /// the limit, including for a zero limit. This lets an embedding stop at a
    /// service entry, fulfill a request between runs, and resume the same machine.
    /// An exit, including a callback's HostYield, returns immediately without
    /// consulting the predicate again. Host callbacks do not retire instructions.
    ///
    /// REP completes its repetition, or returns a guest fault with its partial
    /// progress. This limit is independent of the wrapping CPU instruction counter
    /// and is not a wall-clock or device-latency guarantee. Use `|_| false` when
    /// only the instruction limit and guest exits should stop execution.
    pub fn run(
        &mut self,
        instruction_limit: u64,
        mut should_stop: impl FnMut(&Self) -> bool,
    ) -> Result<RunOutcome> {
        let mut completed = 0;
        loop {
            let stop = if should_stop(self) {
                RunStop::HostRequested
            } else if completed == instruction_limit {
                RunStop::InstructionLimit
            } else {
                match self.step()? {
                    StepOutcome::Completed => {
                        completed += 1;
                        continue;
                    }
                    StepOutcome::Exited(exit) => RunStop::Exited(exit),
                }
            };
            return Ok(RunOutcome { completed, stop });
        }
    }
}
