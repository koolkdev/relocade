use super::{
    dispatch::Dispatch, Cpu, Error, HookResult, Result, RuntimeBackend, RuntimeMemory, RuntimeState,
};
use relocade_wasm_runtime::WasmContext;
use relocade_x86::ExecutionExit;

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
        let mut execution = self.execution();
        let target = execution.instruction_target(instruction_budget);
        execution.enter(target)
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
        let mut execution = self.execution();
        let target = execution.instruction_target(instruction_budget);
        let mut exit = execution.enter(target)?;
        while exit == ExecutionExit::Deferred {
            exit = match execution.run_deferred().await {
                HookResult::Continue => execution.enter(target)?,
                HookResult::Exit(exit) => exit,
            };
        }
        Ok(exit)
    }

    /// Drops a queued handler and clears any interrupted call, without changing CPU state.
    /// Use when replacing a suspended program or its continuation.
    /// Cancellation does not undo side effects already performed by the operation.
    pub fn cancel_pending_call(&mut self) {
        let pending = self.backend.state_mut().pending.take();
        self.host_call_incomplete = false;
        drop(pending);
    }

    fn execution(&mut self) -> Execution<'_, T, B> {
        let mut backend = self.backend.execution();
        backend.state_mut().running = true;
        Execution {
            backend,
            bindings: &self.bindings,
            dispatch: &self.dispatch,
            host_call_incomplete: &mut self.host_call_incomplete,
        }
    }
}

/// Retains the CPU's backend and bindings through guest calls and deferred
/// host work. Each synchronous call lends access to its callbacks; between
/// calls the same owner lends the stopped CPU to the deferred handler.
struct Execution<'a, T: 'static, B: RuntimeBackend<State = RuntimeState<T>> + 'a> {
    backend: B::Execution<'a>,
    bindings: &'a RuntimeMemory<B::Memory, B::SharedMemory>,
    dispatch: &'a Dispatch<B>,
    host_call_incomplete: &'a mut bool,
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Drop for Execution<'_, T, B> {
    fn drop(&mut self) {
        self.backend.state_mut().running = false;
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Execution<'_, T, B> {
    fn instruction_target(&mut self, instruction_budget: u64) -> u64 {
        self.bindings
            .cpu(&mut self.backend)
            .instruction_count()
            .saturating_add(instruction_budget)
    }

    fn enter(&mut self, target: u64) -> Result<ExecutionExit> {
        if *self.host_call_incomplete {
            return Err(Error::CancelledHostCall);
        }
        // A previous synchronous run may already have stopped at a deferred call.
        if self.backend.state().pending.is_some() {
            return Ok(ExecutionExit::Deferred);
        }
        self.bindings
            .set_instruction_target(&mut self.backend, target);
        Ok(self.dispatch.run(&mut self.backend))
    }

    async fn run_deferred(&mut self) -> HookResult {
        let handler = self
            .backend
            .state_mut()
            .pending
            .take()
            .expect("deferred exit must have a handler");
        // Cancellation leaves this set so a later run cannot replay the hook.
        *self.host_call_incomplete = true;
        let mut call = self.bindings.host_call(&mut self.backend);
        let result = handler.run(&mut call).await;
        *self.host_call_incomplete = false;
        result
    }
}
