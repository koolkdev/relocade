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
/// Requests remain pending until observed by a polled run. This handle
/// does not wake suspended execution; session cancellation belongs to its owner.
/// It retains only the request flag, independently of the CPU and backend.
#[derive(Clone)]
pub struct ExitHandle(Rc<Cell<bool>>);

impl ExitHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    pub(super) fn is_requested(&self) -> bool {
        self.0.get()
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

    /// Runs until the guest exits or an execution interrupt is requested,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Guest checkpoints return `Interrupted` independently of IF and preserve
    /// suspended calls. The request remains set until its owner acknowledges it
    /// with `ExecutionInterrupt::take`. Checkpoints occur at completed blocks;
    /// straight-line execution and REP have no fixed notification-latency bound.
    ///
    /// Dropping this waiter preserves active and parked calls and their locals.
    /// A subsequent run continues from the same call or guest position. An observed
    /// exit request releases retained calls, clears HLT and returns `HostExit::Return`.
    /// Dropping the CPU drops retained calls innermost first, before the machine.
    ///
    /// Real-mode device IRQs are delivered at stopped guest boundaries according
    /// to live IF. Pending services establish their live guest resume position
    /// before suspending with IF enabled. IRQ entry saves that CS:IP normally.
    /// A suspended service stays parked until guest reaches a resume entry, even
    /// when its input becomes ready. Reentry starts a separate invocation.
    /// Execution interrupts, guest faults and replacement waiters preserve parked calls.
    /// Guest IF-enabling instructions return to this same IRQ polling loop, so a
    /// request queued while masked needs no later notification to become eligible.
    /// HLT retires and waits without executing its successor until an eligible
    /// device IRQ is accepted. Unrelated wakes and replacement run waiters retain
    /// the halt. Devices and session cancellation remain independently pollable.
    pub async fn run(&mut self) -> ExecutionExit {
        let exit = poll_fn(|cx| {
            if self.exit.is_requested() {
                return Poll::Ready(ExecutionExit::Host(HostExit::Return));
            }
            // Poll active work once; guest entries below poll new or resumed calls.
            if let Some(call) = self.pending.take() {
                self.pending = call.poll(cx);
            }
            loop {
                let interrupt_delivered = match self.try_deliver_interrupt(cx) {
                    Ok(delivered) => delivered,
                    Err(exit) => return Poll::Ready(exit),
                };
                if interrupt_delivered {
                    if let Some(call) = self.pending.take() {
                        self.suspended.push(call);
                    }
                } else if self.pending.is_some() || self.halted {
                    return Poll::Pending;
                }

                let exit = self.run_guest();
                if self.exit.is_requested() {
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
                    | ExecutionExit::Halted => continue,
                    exit => return Poll::Ready(exit),
                };
            }
        })
        .await;
        if exit == ExecutionExit::Host(HostExit::Return) {
            self.pending.take();
            self.abandon_suspended_calls();
            self.halted = false;
            // Coalesce any requests made while destroying the retained work.
            self.exit.0.set(false);
        }
        exit
    }

    /// Delivers an IRQ at a stopped boundary, whether or not a host call is pending.
    /// Reports success only after entry; the caller then parks any active call.
    fn try_deliver_interrupt(&mut self, cx: &mut Context<'_>) -> Result<bool, ExecutionExit> {
        if self.exit.is_requested() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let interrupt = self.machine.poll_interrupt(cx);
        // Device callbacks can request an exit, including when no IRQ is ready.
        if self.exit.is_requested() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let Poll::Ready(vector) = interrupt else {
            return Ok(false);
        };
        // Interrupt recognition ends HLT even if entering the ISR faults.
        self.halted = false;
        let entered = self.machine.enter_interrupt(vector);
        // Frame writes and IVT reads may also request an exit. Observe it before
        // parking a call or executing the ISR, even when IRQ entry faults.
        if self.exit.is_requested() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        entered?;
        Ok(true)
    }
}
