//! The driver alternates guest execution with polling retained host services.

mod budget;
mod entry;
mod guest_call;
mod requests;
mod suspended;

use super::{call::Call, Cpu};
use budget::RunBudget;
pub(super) use guest_call::GuestCall;
use relocade_x86::{Exception, ExecutionExit, HostExit};
pub(super) use requests::GuestCalls;
use std::{cell::Cell, future::poll_fn, rc::Rc, task::Poll};
pub(super) use suspended::SuspendedCall;

/// Requests a return to the embedding after the current guest dispatch or host poll.
/// Capture this with a service's other resources, for example when DOS terminates
/// a program. The driver releases pending work and consumes the request before
/// returning the requested host exit.
///
/// Requests remain pending until observed by a polled run. This handle
/// does not wake suspended execution; session cancellation belongs to its owner.
/// It retains only the requested result, independently of the CPU and backend.
#[derive(Clone)]
pub struct ExitHandle(Rc<Cell<Option<ExecutionExit>>>);

impl ExitHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    /// Requests an execution exit. The current host poll completes normally.
    pub fn request(&self) {
        self.0.set(Some(ExecutionExit::Host(HostExit::Return)));
    }

    /// Reports an exception from its installed default handler and ends execution.
    /// The saved interrupt frame retains the original fault location. Pending
    /// and suspended Rust handlers are released when the driver observes this exit.
    pub fn request_unhandled_exception(&self, exception: Exception) {
        self.0
            .set(Some(ExecutionExit::UnhandledException(exception)));
    }
}

impl Cpu {
    /// Captures an execution-exit flag for host services and device callbacks.
    pub fn exit_handle(&self) -> ExitHandle {
        self.exit.clone()
    }

    /// Runs until the guest exits or an execution interrupt is requested,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Guest checkpoints return `Interrupted` independently of IF and preserve
    /// suspended calls. The request remains set until its owner acknowledges it
    /// with `ExecutionInterrupt::take`. Checkpoints occur at completed blocks;
    /// straight-line execution and REP have no fixed notification-latency bound.
    ///
    /// Dropping this waiter preserves active and parked calls and their locals.
    /// A subsequent run continues from the same call or guest position. An observed
    /// exit request releases retained calls and returns its requested host result.
    /// Dropping the CPU drops retained calls innermost first, before the machine.
    ///
    /// Real-mode device IRQs are delivered at stopped guest boundaries according
    /// to live IF. Pending services establish their live guest resume position
    /// before suspending with IF enabled. IRQ entry saves that CS:IP normally.
    /// A suspended service stays parked until guest reaches a resume entry, even
    /// when its input becomes ready. Reentry starts a separate invocation.
    /// Execution interrupts, undelivered faults and replacement waiters preserve parked calls.
    /// Real16 #DE/#BR/#UD enter their live guest vector before IRQ arbitration;
    /// an unhandled-exception exit from a default handler releases parked calls.
    /// Guest IF-enabling instructions return to this same IRQ polling loop, so a
    /// request queued while masked needs no later notification to become eligible.
    /// Awaited guest interrupt and far calls use the same entry and resume machinery,
    /// taking precedence over device IRQ polling and entering regardless of IF.
    pub async fn run(&mut self) -> ExecutionExit {
        self.run_with_budget(0).await
    }

    /// Like [`Self::run`], with a soft instruction allowance. Zero is unlimited.
    /// Returns [`ExecutionExit::BudgetExhausted`] at a checkpoint, which may overshoot.
    /// The allowance spans service waits and clears on completion or drop. Preserve
    /// the retirement count while a budget is active.
    ///
    /// # Panics
    /// Panics if the allowance exceeds `i64::MAX`, or is nonzero when this CPU's
    /// execution target disables instruction budgets.
    pub async fn run_with_budget(&mut self, instruction_budget: u64) -> ExecutionExit {
        let _budget = RunBudget::new(&self.machine.bindings, instruction_budget);
        let exit = poll_fn(|cx| {
            if let Some(exit) = self.exit.0.get() {
                return Poll::Ready(exit);
            }
            // Poll active work once; guest entries below poll new or resumed calls.
            if let Some(call) = self.pending.take() {
                self.pending = call.poll(cx);
            }
            loop {
                let guest_entered = match self.try_enter_guest(cx) {
                    Ok(delivered) => delivered,
                    Err(exit) => return Poll::Ready(exit),
                };
                if !guest_entered && self.pending.is_some() {
                    return Poll::Pending;
                }

                let exit = self.run_guest();
                if self.exit.0.get().is_some() {
                    continue;
                }
                self.pending = match exit {
                    ExecutionExit::Host(HostExit::Call { service_id }) => {
                        Call::start(self.machine.dispatch.handler(service_id), cx)
                    }
                    ExecutionExit::Host(HostExit::Resume) => match self.resume_call(cx) {
                        Ok(call) => call,
                        Err(exit) => return Poll::Ready(exit),
                    },
                    ExecutionExit::StepComplete
                    | ExecutionExit::InterruptsEnabled
                    | ExecutionExit::InterruptEntered
                    | ExecutionExit::FarCallEntered => continue,
                    exit => return Poll::Ready(exit),
                };
            }
        })
        .await;
        if matches!(
            exit,
            ExecutionExit::Host(HostExit::Return) | ExecutionExit::UnhandledException(_)
        ) {
            self.pending.take();
            self.abandon_suspended_calls();
            // Coalesce any requests made while destroying the retained work.
            self.exit.0.set(None);
        }
        exit
    }

    /// Runs guest code to the next host entry or guest exit, including any
    /// successor protected by an interrupt-inhibiting instruction.
    fn run_guest(&mut self) -> ExecutionExit {
        let machine = &mut self.machine;
        let mut exit = machine.dispatch();
        // Finish protected successors before polling IRQs or returning for a
        // checkpoint. The execution step also delivers supported faults, then
        // publishes its result without dispatching the target.
        while exit == ExecutionExit::InterruptInhibited && self.exit.0.get().is_none() {
            exit = machine.dispatch.step(&mut machine.backend);
        }
        exit
    }
}
