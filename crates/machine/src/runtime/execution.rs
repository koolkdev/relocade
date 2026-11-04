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
    /// exit request releases retained calls and returns `HostExit::Return`.
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
    pub async fn run(&mut self) -> ExecutionExit {
        let exit = poll_fn(|cx| {
            if self.exit.0.get() {
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
                } else if self.pending.is_some() {
                    return Poll::Pending;
                }

                let exit = self.run_guest();
                if self.exit.0.get() {
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
                    ExecutionExit::StepComplete | ExecutionExit::InterruptsEnabled => continue,
                    exit => return Poll::Ready(exit),
                };
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

    /// Delivers an IRQ at a stopped boundary, whether or not a host call is pending.
    /// Reports success only after entry; the caller then parks any active call.
    fn try_deliver_interrupt(&mut self, cx: &mut Context<'_>) -> Result<bool, ExecutionExit> {
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let interrupt = self.machine.poll_interrupt(cx);
        // Device callbacks can request an exit, including when no IRQ is ready.
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let Poll::Ready(vector) = interrupt else {
            return Ok(false);
        };
        let entered = self.machine.enter_interrupt(vector);
        // Frame writes and IVT reads may also request an exit. Observe it before
        // parking a call or executing the ISR, even when IRQ entry faults.
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        entered?;
        Ok(true)
    }

    /// Runs guest code to the next host entry or guest exit, including any
    /// successor protected by an interrupt-inhibiting instruction.
    fn run_guest(&mut self) -> ExecutionExit {
        let machine = &mut self.machine;
        let mut exit = machine.dispatch();
        // Finish protected successors before polling IRQs or returning for a
        // checkpoint. STEP publishes its result without dispatching the target.
        while exit == ExecutionExit::InterruptInhibited && !self.exit.0.get() {
            exit = machine.dispatch.step(&mut machine.backend);
        }
        exit
    }
}
