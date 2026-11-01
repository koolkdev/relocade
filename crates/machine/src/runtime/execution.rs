//! The driver alternates guest execution with polling retained host services.

mod suspended;

use super::{call::Call, Cpu};
use relocade_x86::{ExecutionExit, HostExit};
use std::{
    cell::Cell,
    future::poll_fn,
    rc::Rc,
    task::{Context, Poll},
};

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
    /// active and parked calls and their locals. A subsequent run continues from
    /// the same active call or guest position with a new budget. A zero budget
    /// returns `BudgetExhausted` without entering Wasm or polling pending work.
    /// An observed exit request releases active and parked calls and returns
    /// `HostExit::Return`. Dropping the waiter preserves both the calls and any
    /// unobserved request; a zero budget consumes neither.
    /// Dropping the CPU drops retained calls innermost first, before the machine.
    /// Real-mode device IRQs are delivered at stopped guest boundaries according
    /// to live IF. Pending services establish their live guest resume position
    /// before suspending with IF enabled. IRQ entry saves that CS:IP normally.
    /// A suspended service stays parked until guest reaches a resume entry, even
    /// when its input becomes ready. Reentry starts a separate invocation.
    /// Budget/fault exits and replacement waiters preserve parked calls.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        // A replacement waiter supplies its budget at the next guest dispatch.
        // Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        let exit = poll_fn(|cx| loop {
            if self.exit.0.get() {
                return Poll::Ready(ExecutionExit::Host(HostExit::Return));
            }
            self.pending = if let Some(call) = self.pending.take() {
                call.poll(cx)
            } else {
                let exit = self.run_guest(budget.take(), cx);
                if self.exit.0.get() {
                    continue;
                }
                match exit {
                    ExecutionExit::Host(HostExit::Call { service_id }) => {
                        let hook = self.machine.dispatch.hook(service_id);
                        Call::start(&hook.handler, cx)
                    }
                    ExecutionExit::Host(HostExit::Resume) => match self.resume_call(cx) {
                        Ok(call) => call,
                        Err(exit) => return Poll::Ready(exit),
                    },
                    exit => return Poll::Ready(exit),
                }
            };
            if self.pending.is_some() && !self.exit.0.get() {
                match self.suspend_for_interrupt(cx) {
                    Ok(false) if !self.exit.0.get() => return Poll::Pending,
                    Ok(_) => {}
                    Err(exit) => return Poll::Ready(exit),
                }
            }
        })
        .await;
        if exit == ExecutionExit::Host(HostExit::Return) {
            self.pending.take();
            self.abandon_suspended_calls();
            // Coalesce any requests made while destroying the retained work.
            self.exit.0.set(false);
        }
        exit
    }

    /// Runs from a stopped guest boundary to the next host entry or guest exit.
    /// No retained host call is active while guest interrupts are delivered.
    fn run_guest(&mut self, budget: Option<u64>, cx: &mut Context<'_>) -> ExecutionExit {
        let interrupt = self.machine.poll_interrupt(cx);
        // Device callbacks can request an exit, including when no IRQ is ready.
        if self.exit.0.get() {
            return ExecutionExit::Host(HostExit::Return);
        }
        if let Poll::Ready(vector) = interrupt {
            if let Err(exit) = self.machine.enter_interrupt(vector) {
                return exit;
            }
            // Frame writes and IVT reads may also call devices. Do not enter
            // the ISR after an exit request made by one of those callbacks.
            if self.exit.0.get() {
                return ExecutionExit::Host(HostExit::Return);
            }
        }

        let machine = &mut self.machine;
        let mut exit = machine.dispatch(budget);
        // STI and SS loads must execute their successor before another IRQ poll
        // or budget exit. STEP executes it and resumes ordinary guest dispatch;
        // another inhibiting instruction simply requests another step.
        while exit == ExecutionExit::InterruptInhibited && !self.exit.0.get() {
            exit = machine.dispatch.step(&mut machine.backend);
        }
        exit
    }
}
