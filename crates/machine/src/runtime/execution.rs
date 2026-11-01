//! The driver alternates guest execution with polling retained host services.

use super::{call::Call, Cpu};
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
    /// Stop requests belong to this run from its first poll until it returns or
    /// is dropped. An observed stop releases the pending call and returns
    /// `HostExit::Return`; dropping the waiter instead retains that call.
    /// Dropping the CPU drops its retained call and machine together.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        let running = self.stop.enter();
        // A replacement waiter supplies its budget after the retained call
        // completes. Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        poll_fn(|cx| {
            running.register(cx.waker());
            loop {
                if running.requested() {
                    self.pending.take();
                    return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                }
                self.pending = if let Some(call) = self.pending.take() {
                    call.poll(cx)
                } else {
                    let machine = &mut self.machine;
                    let exit = machine.dispatch(budget.take());
                    if running.requested() {
                        return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                    }
                    let ExecutionExit::Host(HostExit::Call { service_id }) = exit else {
                        return Poll::Ready(exit);
                    };
                    Call::start(machine.services.handler(service_id), cx)
                };
                if self.pending.is_some() && !running.requested() {
                    return Poll::Pending;
                }
            }
        })
        .await
    }
}
