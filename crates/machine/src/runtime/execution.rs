use super::{
    dispatch::DispatchOutcome, Cpu, Error, HostAction, HostOperation, Result, RuntimeBackend,
};
use crate::CpuContext;
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

pub(super) enum Execution {
    Ready,
    Pending(HostOperation),
    Interrupted,
}

impl<B: RuntimeBackend> Cpu<B> {
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
        self.execution = Execution::Ready;
    }

    fn instruction_target(&mut self, instruction_budget: u64) -> u64 {
        self.context()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget)
    }

    fn enter(&mut self, target: u64) -> Result<ExecutionExit> {
        match self.execution {
            Execution::Interrupted => return Err(Error::CancelledHostCall),
            Execution::Pending(_) => return Ok(ExecutionExit::Deferred),
            Execution::Ready => {}
        }
        self.bindings
            .set_instruction_target(&mut self.backend, target);
        loop {
            let action = match self.dispatch.run(&mut self.backend)? {
                DispatchOutcome::Exit(exit) => return Ok(exit),
                DispatchOutcome::Hook(handler) => {
                    let mut context =
                        CpuContext::new(self.bindings.cpu(&mut self.backend), self.memory.view());
                    handler(&mut context)
                }
            };
            if let ControlFlow::Break(exit) = self.apply_action(action) {
                return Ok(exit);
            }
        }
    }

    fn apply_action(&mut self, action: HostAction) -> ControlFlow<ExecutionExit> {
        match action {
            HostAction::Continue => ControlFlow::Continue(()),
            HostAction::Exit(exit) => {
                assert_ne!(
                    exit,
                    ExecutionExit::Deferred,
                    "waiting requires an owned host operation"
                );
                ControlFlow::Break(exit)
            }
            HostAction::Await(operation) => {
                self.execution = Execution::Pending(operation);
                ControlFlow::Break(ExecutionExit::Deferred)
            }
        }
    }

    async fn run_deferred(&mut self) -> ControlFlow<ExecutionExit> {
        // Dropping the run future leaves Interrupted set and drops the operation,
        // so another run cannot replay side effects or enter its unfinished return.
        let Execution::Pending(operation) =
            std::mem::replace(&mut self.execution, Execution::Interrupted)
        else {
            unreachable!("deferred exit must have an owned operation");
        };
        let action = operation.run(&mut self.context()).await;
        self.execution = Execution::Ready;
        self.apply_action(action)
    }
}
