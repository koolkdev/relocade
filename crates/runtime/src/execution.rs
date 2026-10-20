use crate::{
    state::Completion, with_host_call, Error, Result, Runtime, RuntimeBackend, RuntimeState,
};
use relocade_x86::ExecutionExit;
use std::{
    future::poll_fn,
    ops::ControlFlow,
    task::{Context, Poll},
};

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Runs until the budget is consumed at a block boundary, a guest exit
    /// occurs, or a host function requests a return. Whole blocks can exceed the
    /// budget; straight-line code and REP have no fixed overshoot or elapsed-time bound.
    /// Zero does not enter Wasm. The target saturates at u64::MAX.
    ///
    /// A deferred host operation returns HostReturn. Subsequent synchronous runs
    /// also return HostReturn without polling it; use `run_async` to wait and resume.
    pub fn run(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
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
    /// Wasm calls remain synchronous. Pending Rust futures are polled after Wasm
    /// returns. Completions apply their CPU/memory changes directly in Rust before
    /// dispatch resumes at the continuation they set.
    /// Dropping this run future retains the operation for a later `run_async`.
    /// Zero does not enter Wasm or poll pending work.
    pub async fn run_async(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self.instruction_target(instruction_budget);
        loop {
            while let Some(complete) = poll_fn(|cx| self.poll_pending(cx)).await {
                // A completion can defer more work or return to the embedding.
                if self.complete(complete).is_break() && !self.has_pending_call() {
                    return Ok(ExecutionExit::HostReturn);
                }
            }
            let exit = self.enter(target)?;
            if exit != ExecutionExit::HostReturn || !self.has_pending_call() {
                return Ok(exit);
            }
        }
    }

    /// Whether a hook has deferred its completion.
    pub fn has_pending_call(&self) -> bool {
        self.storage.access.state().pending.is_some()
    }

    /// Drops a deferred operation and its completion, without changing CPU state.
    /// Cancel before replacing a suspended program or editing its continuation.
    /// Cancellation does not undo side effects already performed by the operation.
    pub fn cancel_pending_call(&mut self) {
        let pending = self.storage.access.state_mut().pending.take();
        drop(pending);
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

    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<Option<Completion<T>>> {
        // Release the backend's state guard before polling user-owned work.
        let pending = self.storage.access.state_mut().pending.take();
        let Some(mut future) = pending else {
            return Poll::Ready(None);
        };
        match future.as_mut().poll(cx) {
            Poll::Ready(complete) => Poll::Ready(Some(complete)),
            Poll::Pending => {
                self.storage.access.state_mut().pending = Some(future);
                Poll::Pending
            }
        }
    }

    fn complete(&mut self, complete: Completion<T>) -> ControlFlow<()> {
        let config = &self.config;
        let context = self.storage.context.clone();
        let guest = self.storage.guest.clone();
        self.storage.access.with_context(|mut access| {
            with_host_call(config, &mut access, context, guest, complete)
        })
    }
}
