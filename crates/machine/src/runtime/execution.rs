//! Idle machine ownership and retained execution across pending host services.

use super::{machine::Machine, Cpu, HookResult};
use futures_util::future::LocalBoxFuture;
use relocade_x86::{ExecutionExit, HostExit};
use std::{cell::Cell, future::poll_fn, rc::Rc, task::Poll};

pub(super) enum Execution<T> {
    // Taken only while synchronously transferring ownership into Pending.
    Idle(Option<Machine<T>>),
    Pending {
        future: LocalBoxFuture<'static, (Machine<T>, ExecutionExit)>,
        replacement_budget: Rc<Cell<Option<u64>>>,
    },
}

impl<T: 'static> Execution<T> {
    pub fn new(machine: Machine<T>) -> Self {
        Self::Idle(Some(machine))
    }

    pub fn machine(&self) -> &Machine<T> {
        match self {
            Self::Idle(machine) => machine.as_ref().expect("idle machine"),
            Self::Pending { .. } => {
                panic!("host service is suspended; complete it before accessing the machine")
            }
        }
    }

    pub fn machine_mut(&mut self) -> &mut Machine<T> {
        match self {
            Self::Idle(machine) => machine.as_mut().expect("idle machine"),
            Self::Pending { .. } => {
                panic!("host service is suspended; complete it before accessing the machine")
            }
        }
    }

    async fn run(&mut self, budget: u64) -> ExecutionExit {
        if budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        match self {
            Self::Idle(machine) => {
                // Guest-only runs leave the machine here and allocate no future.
                let exit = machine
                    .as_mut()
                    .expect("idle machine")
                    .dispatch(Some(budget));
                let ExecutionExit::Host(HostExit::Call { address }) = exit else {
                    return exit;
                };
                let machine = machine.take().expect("idle machine");
                let replacement_budget = Rc::new(Cell::new(None));
                *self = Self::Pending {
                    future: Box::pin(execute(machine, address, Rc::clone(&replacement_budget))),
                    replacement_budget,
                };
            }
            Self::Pending {
                replacement_budget, ..
            } => replacement_budget.set(Some(budget)),
        }
        poll_fn(|cx| {
            let Self::Pending { future, .. } = self else {
                unreachable!("running host services");
            };
            match future.as_mut().poll(cx) {
                Poll::Ready((machine, exit)) => {
                    *self = Self::Idle(Some(machine));
                    Poll::Ready(exit)
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

async fn execute<T>(
    mut machine: Machine<T>,
    mut address: u32,
    replacement_budget: Rc<Cell<Option<u64>>>,
) -> (Machine<T>, ExecutionExit) {
    let exit = loop {
        match machine.call_host(address).await {
            HookResult::Continue => {}
            HookResult::Exit(exit) => break exit,
        }
        // A replacement waiter supplies one budget, applied after the
        // pending service returns. Ordinary waits do not replenish it.
        match machine.dispatch(replacement_budget.take()) {
            ExecutionExit::Host(HostExit::Call { address: next }) => address = next,
            exit => break exit,
        }
    };
    (machine, exit)
}

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
    /// Dropping the CPU drops the suspended call chain and its machine together.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        self.execution.run(instruction_budget).await
    }
}
