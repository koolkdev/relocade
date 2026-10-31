//! Driver-owned machine and retained host invocations.

use super::{
    access::Scope,
    call::{Call, CallPoll},
    machine::Machine,
    Cpu, HookResult,
};
use relocade_x86::{ExecutionExit, HostExit};
use std::{future::poll_fn, rc::Rc, task::Poll};

pub(super) struct Execution<T: 'static> {
    // Cancellation drops the future without granting access, before its machine.
    call: Option<Call<T>>,
    pub(super) machine: Machine<T>,
    access: Rc<Scope<T>>,
}

impl<T: 'static> Execution<T> {
    pub fn new(machine: Machine<T>) -> Self {
        Self {
            call: None,
            machine,
            access: Rc::new(Scope::new()),
        }
    }

    pub fn machine(&self) -> &Machine<T> {
        self.assert_idle();
        &self.machine
    }

    pub fn machine_mut(&mut self) -> &mut Machine<T> {
        self.assert_idle();
        &mut self.machine
    }

    fn assert_idle(&self) {
        assert!(
            self.call.is_none(),
            "host service is suspended; complete it before accessing the machine"
        );
    }

    async fn run(&mut self, budget: u64) -> ExecutionExit {
        if budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        // A replacement waiter applies its budget after the pending call returns.
        // Further polls of the same waiter do not replenish the target.
        let mut budget = Some(budget);
        poll_fn(|cx| loop {
            let result = if let Some(call) = self.call.take() {
                call.resume(&mut self.machine, &self.access, cx)
            } else {
                match self.machine.dispatch(budget.take()) {
                    ExecutionExit::Host(HostExit::Call { address }) => {
                        let handler = Rc::clone(
                            self.machine
                                .hooks
                                .get(&address)
                                .expect("registered host entry"),
                        );
                        Call::start(&mut self.machine, &self.access, handler, cx)
                    }
                    exit => return Poll::Ready(exit),
                }
            };
            match result {
                CallPoll::Ready(HookResult::Continue) => {}
                CallPoll::Ready(HookResult::Exit(exit)) => return Poll::Ready(exit),
                CallPoll::Pending(call) => {
                    self.call = Some(call);
                    return Poll::Pending;
                }
            }
        })
        .await
    }
}

impl<T: 'static> Cpu<T> {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending invocation and its locals. A subsequent run resumes that call
    /// and supplies a new budget for continued guest execution. A zero budget
    /// returns `BudgetExhausted` without entering Wasm or polling pending work.
    /// CPU access is bound only during each poll and completion cleanup. Dropping
    /// the CPU cancels its pending future without granting it CPU access.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        self.execution.run(instruction_budget).await
    }
}
