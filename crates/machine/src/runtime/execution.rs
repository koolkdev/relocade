//! The driver alternates guest execution with polling retained host services.

mod entry;
mod guest_call;
mod requests;
mod suspended;

use super::{call::Call, Cpu};
pub(super) use guest_call::GuestCall;
use relocade_x86::{Exception, ExecutionExit, HostExit};
pub(super) use requests::GuestCalls;
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    rc::{Rc, Weak},
    task::{ready, Context, Poll, Waker},
};
pub(super) use suspended::SuspendedCall;

/// Requests a return to the embedding after the current guest dispatch or host poll.
/// Capture this with a service's other resources, for example when DOS terminates
/// a program. The driver releases pending work and consumes the request before
/// returning the requested host exit.
///
/// Requests remain pending until observed by a polled run. This handle
/// wakes the machine when bound to a session. Session cancellation belongs
/// to the worker owner. The handle retains no CPU or Wasm runtime.
#[derive(Clone)]
pub struct ExitHandle(Rc<ExitRequest>);

#[derive(Default)]
struct ExitRequest {
    result: Cell<Option<ExecutionExit>>,
    machine: RefCell<Weak<Waker>>,
}

impl ExitHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    fn result(&self) -> Option<ExecutionExit> {
        self.0.result.get()
    }

    pub(super) fn is_requested(&self) -> bool {
        self.result().is_some()
    }

    pub(crate) fn bind(&self, machine: &Waker) -> Rc<Waker> {
        let machine = Rc::new(machine.clone());
        *self.0.machine.borrow_mut() = Rc::downgrade(&machine);
        machine
    }

    fn set(&self, exit: ExecutionExit) {
        self.0.result.set(Some(exit));
        if let Some(machine) = self.0.machine.borrow().upgrade() {
            machine.wake_by_ref();
        }
    }

    /// Requests an execution exit. The current host poll completes normally.
    pub fn request(&self) {
        self.set(ExecutionExit::Host(HostExit::Return));
    }

    /// Reports an exception from its installed default handler and ends execution.
    /// The saved interrupt frame retains the original fault location. Pending
    /// and suspended Rust handlers are released when the driver observes this exit.
    pub fn request_unhandled_exception(&self, exception: Exception) {
        self.set(ExecutionExit::UnhandledException(exception));
    }
}

impl Cpu {
    /// Captures an execution-exit flag for host services and device callbacks.
    pub fn exit_handle(&self) -> ExitHandle {
        self.exit.clone()
    }

    /// Runs until the guest exits, its instruction target is reached, or an
    /// execution interrupt is requested, awaiting host services as needed. Ready services complete in the same poll.
    /// Guest checkpoints return `Interrupted` independently of IF and preserve
    /// suspended calls. The request remains set until its owner acknowledges it
    /// with `ExecutionInterrupt::take`. Checkpoints occur at completed blocks;
    /// straight-line execution and REP have no fixed notification-latency bound.
    ///
    /// Reaching the instruction target returns `BudgetExhausted`; the external
    /// controller owns target updates and cleanup, independently of this waiter.
    ///
    /// Dropping this waiter preserves active and parked calls and their locals.
    /// A subsequent run continues from the same call or guest position. An observed
    /// exit request releases retained calls, clears HLT and returns its requested result.
    /// Dropping the CPU drops retained calls innermost first, before the CPU instance.
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
    /// HLT retires and waits without executing its successor until an eligible
    /// device IRQ is accepted. Unrelated wakes and replacement run waiters retain
    /// the halt. Devices and session cancellation remain independently pollable.
    pub async fn run(&mut self) -> ExecutionExit {
        poll_fn(|cx| {
            let _execution = ready!(self.poll_enter_execution(cx));
            match self.poll_host_work(cx) {
                Some(exit) => Poll::Ready(exit),
                None => self.poll_execution(cx, false),
            }
        })
        .await
    }

    /// Enters one synchronous CPU turn, including service polling and IRQ entry.
    /// While paused, registers a wakeup for resumption and returns Pending.
    /// Runners retain the returned guard through both poll operations.
    pub(crate) fn poll_enter_execution(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::machine::execution::Active>> {
        match &self.execution {
            Some(execution) => execution.poll_enter(cx).map(Some),
            None => Poll::Ready(None),
        }
    }

    /// Polls the active host service once and observes exit requests without
    /// entering guest code. Pending and parked calls remain owned by the CPU.
    /// `None` means no exit; it does not imply that guest execution is runnable.
    pub(crate) fn poll_host_work(&mut self, cx: &mut Context<'_>) -> Option<ExecutionExit> {
        if !self.exit.is_requested() {
            if let Some(call) = self.pending.take() {
                self.pending = call.poll(cx);
            }
        }
        if let Some(exit) = self.exit.result() {
            return Some(self.finish_exit(exit));
        }
        self.service_compiler();
        None
    }

    /// Enters guest calls or IRQs and dispatches instructions until a checkpoint,
    /// exit or pending service stops execution. New and resumed host calls are
    /// polled at their entries; existing host work is polled by `poll_host_work`.
    /// In a session, return at host boundaries when an activity requests a turn.
    pub(crate) fn poll_execution(
        &mut self,
        cx: &mut Context<'_>,
        in_session: bool,
    ) -> Poll<ExecutionExit> {
        let exit = loop {
            let guest_entered = match self.try_enter_guest(cx) {
                Ok(delivered) => delivered,
                Err(exit) => break exit,
            };
            if !guest_entered && (self.pending.is_some() || self.halted) {
                return Poll::Pending;
            }

            let exit = self.run_guest();
            if self.exit.is_requested() {
                continue;
            }
            match exit {
                ExecutionExit::Host(HostExit::Call { service_id }) => {
                    self.pending = Call::start(self.instance.handler(service_id), cx);
                }
                ExecutionExit::Host(HostExit::Resume) => match self.resume_call(cx) {
                    Ok(call) => self.pending = call,
                    Err(exit) => break exit,
                },
                ExecutionExit::StepComplete
                | ExecutionExit::InterruptsEnabled
                | ExecutionExit::InterruptEntered
                | ExecutionExit::FarCallEntered
                | ExecutionExit::Halted => {}
                exit => break exit,
            }
            // Device I/O or a Rust hook can wake a sibling before guest code
            // reaches a block-end checkpoint, including on the way into HLT.
            if in_session && self.instance.bindings.interrupt.is_requested() {
                break self.exit.result().unwrap_or(ExecutionExit::Interrupted);
            }
        };
        // A session consumes its scheduling interrupts internally. If both
        // checkpoints fired, deliver the caller's instruction target.
        if in_session
            && exit == ExecutionExit::Interrupted
            && self
                .instruction_target
                .reached(self.instance.bindings.cpu.instruction_count())
        {
            return Poll::Ready(ExecutionExit::BudgetExhausted);
        }
        Poll::Ready(self.finish_exit(exit))
    }

    fn finish_exit(&mut self, exit: ExecutionExit) -> ExecutionExit {
        if matches!(
            exit,
            ExecutionExit::Host(HostExit::Return) | ExecutionExit::UnhandledException(_)
        ) {
            self.pending.take();
            self.abandon_suspended_calls();
            self.halted = false;
            // Coalesce any requests made while destroying the retained work.
            self.exit.0.result.set(None);
        }
        exit
    }
}
