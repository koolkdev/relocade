//! The driver alternates guest execution with polling retained host services.

use super::{
    call::{Call, CallPoll},
    Cpu, HookResult,
};
use relocade_x86::{ExecutionExit, HostExit};
use std::{future::poll_fn, task::Poll};

pub(super) struct Frame {
    service_id: u32,
    call: Call,
}

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
            let (service_id, polled) = if let Some(frame) = self.pending.take() {
                (frame.service_id, frame.call.poll(cx))
            } else {
                let exit = machine.dispatch(budget.take());
                let ExecutionExit::Host(HostExit::Call { service_id }) = exit else {
                    return Poll::Ready(exit);
                };
                (
                    service_id,
                    Call::start(&machine.services.get(service_id).handler, cx),
                )
            };
            match polled {
                CallPoll::Ready(HookResult::Complete) => {
                    let complete = machine
                        .services
                        .get(service_id)
                        .continuations
                        .expect("Complete requires service continuations")
                        .complete;
                    if let Err(exception) = machine.bindings.cpu.continue_at(complete) {
                        return Poll::Ready(ExecutionExit::Exception(exception));
                    }
                }
                CallPoll::Ready(HookResult::Continue) => {}
                CallPoll::Ready(HookResult::Exit(exit)) => return Poll::Ready(exit),
                CallPoll::Pending(call) => {
                    self.pending = Some(Frame { service_id, call });
                    return Poll::Pending;
                }
            }
        })
        .await
    }
}
