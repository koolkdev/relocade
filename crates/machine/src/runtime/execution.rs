//! The driver alternates guest execution with polling retained host services.

use super::{
    call::{Call, CallPoll},
    Cpu, HookResult,
};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{
    future::poll_fn,
    task::{Context, Poll},
};

pub(super) struct Frame {
    service_id: u32,
    call: Call,
}

pub(super) struct Suspended {
    frame: Frame,
    /// Original linear code position, interpreted through the live CS on resume.
    code_position: u32,
}

impl Cpu {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. Protected successors execute before
    /// budget exits. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// active and suspended services. A replacement run supplies a new budget.
    /// A zero budget returns without entering Wasm or polling pending work.
    ///
    /// A pending service with continuations permits IRQ delivery according to
    /// live IF and Real16 inhibition. Its future stays parked throughout guest
    /// ISR execution and nested calls. Only a resume entry resumes the top frame, even
    /// when its awaited input becomes ready earlier. Reentry at the service's
    /// original hook always starts a separate invocation.
    ///
    /// IRQs are polled at stopped boundaries. Guest Wasm still runs to its
    /// existing exits; this adds no attention checkpoints or elapsed-time bound.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        // A replacement waiter supplies its budget on the next guest dispatch.
        // Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        let exit = poll_fn(|cx| loop {
            let (service_id, polled) = if let Some(frame) = self.pending.take() {
                (frame.service_id, frame.call.poll(cx))
            } else {
                let machine = &mut self.machine;
                if let Poll::Ready(vector) = machine.poll_interrupt(cx) {
                    if let Err(exit) = machine.enter_interrupt(vector) {
                        return Poll::Ready(exit);
                    }
                }
                let mut exit = machine.dispatch(budget.take());
                loop {
                    match exit {
                        ExecutionExit::Host(HostExit::Call { service_id }) => {
                            break (
                                service_id,
                                Call::start(&machine.services.get(service_id).handler, cx),
                            );
                        }
                        ExecutionExit::Host(HostExit::Resume) => {
                            let Some(top) = self.suspended.last() else {
                                return Poll::Ready(ExecutionExit::Host(HostExit::Resume));
                            };
                            // Only code-position bookkeeping is restored. The ISR's
                            // live CS alias, flags, stack, registers and memory survive.
                            if let Err(exception) =
                                machine.bindings.cpu.continue_at(top.code_position)
                            {
                                return Poll::Ready(ExecutionExit::Exception(exception));
                            }
                            let top = self.suspended.pop().expect("suspended service");
                            break (top.frame.service_id, top.frame.call.poll(cx));
                        }
                        ExecutionExit::InterruptInhibited => {
                            // Execute the successor before IRQ polling or a budget exit.
                            // INT/CALL then dispatch their target with normal eligibility.
                            exit = machine.dispatch.step(&mut machine.backend);
                        }
                        exit => return Poll::Ready(exit),
                    }
                }
            };
            match polled {
                CallPoll::Ready(HookResult::Complete) => {
                    let machine = &self.machine;
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
                    match self.suspend_for_interrupt(cx) {
                        Ok(true) => {}
                        Ok(false) => return Poll::Pending,
                        Err(exit) => return Poll::Ready(exit),
                    }
                }
            }
        })
        .await;
        if exit == ExecutionExit::Host(HostExit::Return) {
            self.abandon_suspended_calls();
        }
        exit
    }

    /// Abandons all parked host calls, innermost first, after guest code has
    /// redirected execution away from their resume entries. This does not cancel
    /// an active pending service or change CPU state. The embedding must ensure
    /// abandoned resume entries will no longer be used to return to those calls.
    /// Session return and dropping the CPU also release parked work.
    pub fn abandon_suspended_calls(&mut self) {
        while self.suspended.pop().is_some() {}
    }

    fn suspend_for_interrupt(&mut self, cx: &mut Context<'_>) -> Result<bool, ExecutionExit> {
        let frame = self.pending.as_ref().expect("pending service");
        let machine = &mut self.machine;
        let Some(code) = machine.services.get(frame.service_id).continuations else {
            // A bare hook cannot supply a return position. Leave the request
            // queued with its device until this call completes.
            return Ok(false);
        };
        let Poll::Ready(vector) = machine.poll_interrupt(cx) else {
            return Ok(false);
        };
        // The request is now acknowledged. A bad return position or an
        // architectural entry fault reports an exit; neither requeues it.
        let cpu = &machine.bindings.cpu;
        let original_ip = cpu.eip();
        let code_position = cpu.segment_base(Segment::Cs).wrapping_add(original_ip);
        if let Err(exception) = cpu.continue_at(code.resume) {
            cpu.set_eip(original_ip);
            return Err(ExecutionExit::Exception(exception));
        }
        if let Err(exit) = machine.enter_interrupt(vector) {
            // Entry did not transfer execution. Undo only our temporary return
            // position; architectural partial effects belong to the fault.
            machine.bindings.cpu.set_eip(original_ip);
            return Err(exit);
        }
        self.suspended.push(Suspended {
            frame: self.pending.take().expect("interrupted service"),
            code_position,
        });
        Ok(true)
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        // Drop the innermost future first, before its parents and the backend.
        self.pending.take();
        self.abandon_suspended_calls();
    }
}
