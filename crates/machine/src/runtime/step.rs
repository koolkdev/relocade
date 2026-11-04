//! Explicit interpreter progress for debuggers and bounded instruction tests.

use super::Cpu;
use relocade_x86::ExecutionExit;

impl Cpu {
    /// Executes the current instruction through the interpreter and publishes
    /// its state. Does not dispatch installed hooks or poll device IRQs. REP
    /// completes as one instruction; faults preserve its completed elements.
    ///
    /// STI and SS loads also execute their protected successor before returning,
    /// including another protected successor if that instruction inhibits IRQs.
    /// This keeps interrupt inhibition within the execution operation, so a later
    /// `run` can safely poll IRQs. Completion returns `StepComplete` or
    /// `InterruptsEnabled` when a flag restore enables IF. An execution-interrupt
    /// request takes precedence and returns `Interrupted` at the completed boundary.
    /// Execution-interrupt requests remain pending for the execution owner.
    ///
    /// # Panics
    /// Panics while a host call is active or suspended. Use `run` to continue
    /// services; instruction stepping must not bypass their resume entries.
    pub fn step(&mut self) -> ExecutionExit {
        assert!(
            self.pending.is_none() && self.suspended.is_empty(),
            "complete host calls before instruction stepping"
        );
        loop {
            let exit = self.machine.dispatch.step(&mut self.machine.backend);
            if exit != ExecutionExit::InterruptInhibited {
                return exit;
            }
        }
    }
}
