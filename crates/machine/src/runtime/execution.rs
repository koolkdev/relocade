//! The driver alternates guest execution with polling retained host services.

use super::{call::Call, Cpu};
use relocade_x86::{ExecutionExit, HostExit};
use std::{cell::Cell, future::poll_fn, rc::Rc, task::Poll};

/// Requests a return to the embedding after the current guest dispatch or host poll.
/// Capture this with a service's other resources, for example when DOS terminates
/// a program. The driver releases pending work and consumes the request before
/// returning `ExecutionExit::Host(HostExit::Return)`.
///
/// Requests remain pending until observed by a nonzero-budget run. This handle
/// does not wake suspended execution; session cancellation belongs to its owner.
/// It retains only the request flag, independently of the CPU and backend.
#[derive(Clone)]
pub struct ExitHandle(Rc<Cell<bool>>);

impl ExitHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    /// Requests an execution exit. The current host poll completes normally.
    pub fn request(&self) {
        self.0.set(true);
    }
}

impl Cpu {
    /// Captures an execution-exit flag for host services and device callbacks.
    pub fn exit_handle(&self) -> ExitHandle {
        self.exit.clone()
    }

    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending service and its locals. A subsequent run resumes that service
    /// and supplies a new budget for continued guest execution. A zero budget
    /// returns `BudgetExhausted` without entering Wasm or polling pending work.
    /// An observed exit request releases the pending call and returns
    /// `HostExit::Return`. Dropping the waiter preserves both the call and any
    /// unobserved request; a zero budget consumes neither.
    /// Dropping the CPU drops its retained call and machine together.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        // A replacement waiter supplies its budget after the retained call
        // completes. Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        poll_fn(|cx| loop {
            if self.exit.0.get() {
                self.pending.take();
                // Destruction may also request an exit; consume it with this one.
                self.exit.0.set(false);
                return Poll::Ready(ExecutionExit::Host(HostExit::Return));
            }
            self.pending = if let Some(call) = self.pending.take() {
                call.poll(cx)
            } else {
                let machine = &mut self.machine;
                let exit = machine.dispatch(budget.take());
                if self.exit.0.get() {
                    continue;
                }
                let ExecutionExit::Host(HostExit::Call { service_id }) = exit else {
                    return Poll::Ready(exit);
                };
                Call::start(machine.services.handler(service_id), cx)
            };
            if self.pending.is_some() && !self.exit.0.get() {
                return Poll::Pending;
            }
        })
        .await
    }
}
