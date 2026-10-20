use crate::{
    callback::DeferredCall, Error, HostCall, Result, Runtime, RuntimeBackend, RuntimeState, Storage,
};
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Runs until the budget is consumed at a block boundary, a guest exit
    /// occurs, or a host function requests a return. Whole blocks can exceed the
    /// budget; straight-line code and REP have no fixed overshoot or elapsed-time bound.
    /// Zero does not enter Wasm. The target saturates at u64::MAX.
    ///
    /// A deferred host operation returns HostReturn. Subsequent synchronous runs
    /// also return HostReturn without polling it; use `run_async` to wait and resume.
    /// An interrupted async handler returns `Error::CancelledHostCall` until
    /// the embedding replaces its continuation and clears the pending call.
    pub fn run(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        self.check_host_call()?;
        if self.has_pending_call() {
            return Ok(ExecutionExit::HostReturn);
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
        self.check_host_call()?;
        let target = self.instruction_target(instruction_budget);
        loop {
            // Release the backend's state guard before borrowing it for the handler.
            let pending = self.storage.access.state_mut().pending.take();
            if let Some(handler) = pending {
                if self.run_deferred(handler).await.is_break() && !self.has_pending_call() {
                    return Ok(ExecutionExit::HostReturn);
                }
                continue;
            }
            let exit = self.enter(target)?;
            if exit != ExecutionExit::HostReturn || !self.has_pending_call() {
                return Ok(exit);
            }
        }
    }

    /// Whether a hook has a queued or interrupted async handler.
    pub fn has_pending_call(&self) -> bool {
        self.host_call_incomplete || self.storage.access.state().pending.is_some()
    }

    /// Drops a queued handler and clears any interrupted call, without changing CPU state.
    /// Use when replacing a suspended program or its continuation.
    /// Cancellation does not undo side effects already performed by the operation.
    pub fn cancel_pending_call(&mut self) {
        let pending = self.storage.access.state_mut().pending.take();
        self.host_call_incomplete = false;
        drop(pending);
    }

    fn check_host_call(&self) -> Result<()> {
        if self.host_call_incomplete {
            Err(Error::CancelledHostCall)
        } else {
            Ok(())
        }
    }

    fn instruction_target(&mut self, instruction_budget: u64) -> u64 {
        self.machine()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget)
    }

    fn enter(&mut self, target: u64) -> Result<ExecutionExit> {
        self.storage.set_instruction_target(target);
        let word = self.storage.access.call(&self.dispatch)? as u64;
        ExecutionExit::from_bits(word).ok_or(Error::InvalidExit(word))
    }

    async fn run_deferred(&mut self, handler: Box<dyn DeferredCall<T>>) -> ControlFlow<()> {
        // Cancellation leaves this set so a later run cannot replay the hook.
        self.host_call_incomplete = true;
        let context = self.storage.context.clone();
        let guest = self.storage.guest.clone();
        let mut access = self.storage.access.context();
        let mut storage = Storage {
            access: &mut access,
            context,
            guest,
        };
        let mut call = HostCall::new(&self.config, &mut storage);
        let result = handler.run(&mut call).await;
        self.host_call_incomplete = false;
        result
    }
}
