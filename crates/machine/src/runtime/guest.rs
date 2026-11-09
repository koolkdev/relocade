//! Synchronous guest execution, protected successors and halt transitions.

use super::Cpu;
use relocade_x86::ExecutionExit;

impl Cpu {
    /// Executes the current instruction through the interpreter and publishes
    /// its state. Does not dispatch installed hooks or poll device IRQs. REP
    /// completes as one instruction; faults preserve its completed elements.
    /// Supported Real16 faults enter the IVT handler and return `InterruptEntered`
    /// without retiring the faulting instruction or executing the handler.
    ///
    /// STI and SS loads also execute their protected successor before returning,
    /// including another protected successor if that instruction inhibits IRQs.
    /// This keeps interrupt inhibition within the execution operation, so a later
    /// `run` can safely poll IRQs. Completion returns `StepComplete` or
    /// `InterruptsEnabled` when a flag restore enables IF. An execution-interrupt
    /// request takes precedence and returns `Interrupted` at the completed boundary.
    /// Execution-interrupt requests remain pending for the execution owner.
    /// HLT returns `Halted`, including when an execution interrupt is pending.
    /// Further steps remain halted; use `run` to accept a device IRQ and resume.
    ///
    /// # Panics
    /// Panics while a host call is active or suspended. Use `run` to continue
    /// services; instruction stepping must not bypass their resume entries.
    pub fn step(&mut self) -> ExecutionExit {
        assert!(
            self.pending.is_none() && self.suspended.is_empty(),
            "complete host calls before instruction stepping"
        );
        if self.halted {
            return ExecutionExit::Halted;
        }
        let exit = self.machine.dispatch.step(&mut self.machine.backend);
        self.finish_guest_exit(exit)
    }

    pub(super) fn run_guest(&mut self) -> ExecutionExit {
        let exit = self.machine.dispatch();
        self.finish_guest_exit(exit)
    }

    /// Finish protected successors before polling IRQs or returning to Rust.
    /// Both entry paths retain HLT before exposing a completed guest boundary.
    fn finish_guest_exit(&mut self, mut exit: ExecutionExit) -> ExecutionExit {
        while exit == ExecutionExit::InterruptInhibited && !self.exit.is_requested() {
            exit = self.machine.dispatch.step(&mut self.machine.backend);
        }
        if exit == ExecutionExit::Halted {
            self.halted = true;
        }
        exit
    }
}
