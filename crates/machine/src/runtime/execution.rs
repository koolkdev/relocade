use super::{Cpu, CpuAccess, Error, HostCall, Result, RuntimeBackend, RuntimeState};
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Cpu<B> {
    /// Runs until the budget is consumed at a block boundary, a guest exit
    /// occurs, or a host function requests a return. Whole blocks can exceed the
    /// budget; straight-line code and REP have no fixed overshoot or elapsed-time bound.
    /// Zero does not enter Wasm. The target saturates at u64::MAX.
    ///
    /// A deferred host operation returns Deferred. Subsequent synchronous runs
    /// also return Deferred without polling it; use `run_async` to wait and resume.
    /// An interrupted async handler returns `Error::CancelledHostCall` until
    /// the embedding replaces its continuation and clears the pending call.
    pub fn run(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self.instruction_target(instruction_budget);
        self.enter(target)
    }

    /// Runs with the same instruction budget as `run`, waiting for deferred host
    /// operations before continuing the guest. Waits do not replenish the budget.
    /// Other host returns and guest exits are returned to the embedding.
    ///
    /// Generated guest calls remain synchronous. Async handlers borrow the
    /// stopped runtime after the guest call returns, then dispatch resumes at
    /// the continuation they set. Keep this same future alive across waits.
    ///
    /// Dropping it during an async handler cancels that handler without undoing
    /// its side effects. Further nonzero runs return `Error::CancelledHostCall`.
    /// Before resuming, replace the guest continuation (for example, load a new
    /// program) and clear the interrupted call with `cancel_pending_call`.
    /// Zero does not enter Wasm or poll pending work.
    pub async fn run_async(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self.instruction_target(instruction_budget);
        let mut exit = self.enter(target)?;
        while exit == ExecutionExit::Deferred {
            exit = match self.run_deferred().await {
                ControlFlow::Continue(()) => self.enter(target)?,
                ControlFlow::Break(exit) => exit,
            };
        }
        Ok(exit)
    }

    /// Drops a queued handler and clears any interrupted call, without changing CPU state.
    /// Use when replacing a suspended program or its continuation.
    /// Cancellation does not undo side effects already performed by the operation.
    pub fn cancel_pending_call(&mut self) {
        let pending = self.access.backend.state_mut().pending.take();
        self.host_call_incomplete = false;
        drop(pending);
    }

    fn instruction_target(&mut self, instruction_budget: u64) -> u64 {
        self.context()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget)
    }

    fn enter(&mut self, target: u64) -> Result<ExecutionExit> {
        if self.host_call_incomplete {
            return Err(Error::CancelledHostCall);
        }
        // A previous synchronous run may already have stopped at a deferred call.
        if self.access.backend.state().pending.is_some() {
            return Ok(ExecutionExit::Deferred);
        }
        self.dispatch.run(&mut self.access, target)
    }

    async fn run_deferred(&mut self) -> ControlFlow<ExecutionExit> {
        // Release the backend's state guard before borrowing it for the handler.
        let handler = self
            .access
            .backend
            .state_mut()
            .pending
            .take()
            .expect("deferred exit must have a handler");
        // Cancellation leaves this set so a later run cannot replay the hook.
        self.host_call_incomplete = true;
        let bindings = self.access.bindings.clone();
        let mut context = self.access.backend.context();
        let mut access = CpuAccess {
            backend: &mut context,
            bindings,
        };
        let mut call = HostCall::new(&self.config, &self.machine_config, &mut access);
        let result = handler.run(&mut call).await;
        self.host_call_incomplete = false;
        result
    }
}
