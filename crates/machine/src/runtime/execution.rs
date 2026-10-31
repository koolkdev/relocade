//! The driver alternates guest execution with polling retained host services.

use super::{
    call::{Call, CallPoll},
    Cpu, HookResult,
};
use relocade_x86::{ExecutionExit, HostExit};
use std::{future::poll_fn, task::Poll};

impl Cpu {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending service and its locals. A subsequent run resumes that service
    /// and supplies a new budget for continued guest execution. A zero budget
    /// returns `BudgetExhausted` without entering Wasm or polling pending work.
    /// Dropping the CPU drops the suspended call chain and its machine together.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        // A replacement waiter supplies its budget after the retained call
        // completes. Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        poll_fn(|cx| loop {
            let machine = &mut self.machine;
            let polled = if let Some(call) = self.pending.take() {
                call.poll(cx)
            } else {
                let exit = machine.dispatch(budget.take());
                let ExecutionExit::Host(HostExit::Call { address }) = exit else {
                    return Poll::Ready(exit);
                };
                Call::start(
                    machine.hooks.get(&address).expect("registered host entry"),
                    cx,
                )
            };
            match polled {
                CallPoll::Ready(HookResult::Continue) => {}
                CallPoll::Ready(HookResult::Exit(exit)) => return Poll::Ready(exit),
                CallPoll::Pending(call) => {
                    self.pending = Some(call);
                    return Poll::Pending;
                }
            }
        })
        .await
    }
}
