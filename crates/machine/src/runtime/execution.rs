//! A retained async call chain and ownership transfer at run boundaries.

use super::{machine::Machine, Cpu};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::Pin,
    rc::Rc,
    task::Poll,
};

pub(super) struct Execution<T> {
    machine: Option<Box<Machine<T>>>,
    task: Pin<Box<dyn Future<Output = ()>>>,
    control: Rc<Control<T>>,
}

struct Control<T> {
    start: Cell<Option<Start<T>>>,
    completed: Cell<Option<Completed<T>>>,
    budget: Cell<Option<u64>>,
}

struct Completed<T> {
    machine: Box<Machine<T>>,
    exit: ExecutionExit,
}

struct Start<T> {
    machine: Box<Machine<T>>,
    address: u32,
}

impl<T: 'static> Execution<T> {
    pub fn new(machine: Machine<T>) -> Self {
        let control = Rc::new(Control {
            start: Cell::new(None),
            completed: Cell::new(None),
            budget: Cell::new(None),
        });
        Self {
            machine: Some(Box::new(machine)),
            task: Box::pin(execute(Rc::clone(&control))),
            control,
        }
    }

    pub fn machine(&self) -> &Machine<T> {
        self.machine
            .as_ref()
            .expect("host service is suspended; complete it before accessing the machine")
    }

    pub fn machine_mut(&mut self) -> &mut Machine<T> {
        self.machine
            .as_mut()
            .expect("host service is suspended; complete it before accessing the machine")
    }

    pub fn is_suspended(&self) -> bool {
        self.machine.is_none()
    }

    async fn run(&mut self, budget: u64) -> ExecutionExit {
        if budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        self.control.budget.set(Some(budget));
        if let Some(machine) = &mut self.machine {
            // Guest execution needs no retained async call chain until it
            // reaches a host service. Budget-only runs keep ownership here.
            let exit = machine.dispatch(self.control.budget.take());
            let ExecutionExit::Host(HostExit::Call { address }) = exit else {
                return exit;
            };
            self.control.start.set(Some(Start {
                machine: self.machine.take().expect("stopped machine"),
                address,
            }));
        }
        poll_fn(|cx| {
            // The task waits for either host input or the next run request. This
            // caller drives it directly, so starting a run needs no separate wake.
            assert!(self.task.as_mut().poll(cx).is_pending());
            if let Some(completed) = self.control.completed.take() {
                self.machine = Some(completed.machine);
                Poll::Ready(completed.exit)
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

async fn execute<T>(control: Rc<Control<T>>) {
    loop {
        let start = poll_fn(|_| control.start.take().map_or(Poll::Pending, Poll::Ready)).await;
        let mut machine = start.machine;
        let exit = machine
            .resume_host_call(start.address, &control.budget)
            .await;
        control.completed.set(Some(Completed { machine, exit }));
    }
}

impl<T: 'static> Cpu<T> {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending service and its locals. A subsequent run resumes that service
    /// and supplies a new budget for continued guest execution. Zero neither
    /// enters Wasm nor polls pending work. Dropping the CPU drops the suspended
    /// call chain and its machine together.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        self.execution.run(instruction_budget).await
    }

    /// Whether a host service retains the machine across an input wait.
    /// Shared guest memory and independent input handles remain accessible.
    pub fn has_pending_call(&self) -> bool {
        self.execution.is_suspended()
    }
}
