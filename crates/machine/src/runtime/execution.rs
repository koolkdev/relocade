//! Execution of guest code and retained host calls on a stable machine owner.

use super::{Call, Cpu, HookResult};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    future::poll_fn,
    task::{ready, Poll},
};

impl<T: 'static> Cpu<T> {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending service and its locals. A subsequent run resumes that service
    /// and supplies a new budget for continued guest execution. A zero budget
    /// returns `BudgetExhausted` without entering Wasm or polling pending work.
    /// Dropping the CPU drops its pending call and machine together.
    ///
    /// Connected devices are polled at the start of each nonzero run
    /// and when a host service completes and continues guest dispatch, subject
    /// to IF. Pending host services finish before that interrupt check.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        let mut budget = Some(instruction_budget);
        poll_fn(|cx| {
            loop {
                if let Some(call) = &mut self.active_call {
                    let result = ready!(call.poll(cx));
                    // Release the invocation before guest execution resumes.
                    self.active_call = None;
                    if let HookResult::Exit(exit) = result {
                        return Poll::Ready(exit);
                    }
                }
                // Each run supplies one budget; ordinary wakes keep the remaining one.
                let exit = self.machine.borrow_mut().dispatch(budget.take());
                match exit {
                    ExecutionExit::Host(HostExit::Call { address }) => {
                        let handler = self
                            .machine
                            .borrow()
                            .hooks
                            .get(&address)
                            .expect("registered host entry")
                            .clone();
                        self.active_call = Some(Call::start(&self.machine, handler));
                    }
                    exit => return Poll::Ready(exit),
                }
            }
        })
        .await
    }
}
