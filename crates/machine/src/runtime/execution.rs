//! The driver alternates guest execution with polling retained host services.

use super::{call::Call, Cpu};
use relocade_x86::{ExecutionExit, HostExit};
use std::{future::poll_fn, task::Poll};

pub(super) struct Frame {
    service_id: u32,
    call: Call,
}

impl Cpu {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. Protected successors execute before
    /// budget exits. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// the pending service and its locals. A subsequent run resumes that service
    /// and supplies a new budget for continued guest execution. A zero budget
    /// returns `BudgetExhausted` without entering Wasm, polling pending work or
    /// consuming a stop request. Stop requests are observed at poll entry and
    /// after guest execution, host polls and IRQ polling, before continuing.
    /// Dropping the CPU drops the suspended call chain and its machine together.
    /// Device IRQs are acknowledged at guest dispatch boundaries, subject to live
    /// IF and Real16 inhibition. Guest Wasm still runs to its existing exits;
    /// this does not add attention checkpoints or an elapsed-time bound.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        let stop = self.stop.clone();
        let _waiter = stop.waiter();
        // A replacement waiter supplies its budget after the retained call
        // completes. Re-polling this same waiter never replenishes the budget.
        let mut budget = Some(instruction_budget);
        let exit = poll_fn(|cx| {
            if stop.requested() {
                return Poll::Ready(ExecutionExit::Host(HostExit::Return));
            }
            loop {
                let machine = &mut self.machine;
                let (service_id, polled) = if let Some(frame) = self.pending.take() {
                    (frame.service_id, frame.call.poll(cx))
                } else {
                    let interrupt = machine.poll_interrupt(cx);
                    if stop.requested() {
                        return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                    }
                    if let Poll::Ready(vector) = interrupt {
                        let entered = machine.enter_interrupt(vector);
                        if stop.requested() {
                            return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                        }
                        if let Err(exit) = entered {
                            return Poll::Ready(exit);
                        }
                    }
                    let mut exit = machine.dispatch(budget.take());
                    loop {
                        if stop.requested() {
                            return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                        }
                        match exit {
                            ExecutionExit::Host(HostExit::Call { service_id }) => {
                                break (
                                    service_id,
                                    Call::start(&machine.services.get(service_id).handler, cx),
                                );
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
                if stop.requested() {
                    // A newly pending call is still local. Drop it before the
                    // driver releases any older suspended work.
                    drop(polled);
                    return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                }
                match polled {
                    None => {
                        if let Some(code) = machine.services.get(service_id).continuations {
                            if let Err(exception) = machine.bindings.cpu.continue_at(code.complete)
                            {
                                return Poll::Ready(ExecutionExit::Exception(exception));
                            }
                        }
                    }
                    Some(call) => {
                        self.pending = Some(Frame { service_id, call });
                        stop.register(cx.waker());
                        if stop.requested() {
                            return Poll::Ready(ExecutionExit::Host(HostExit::Return));
                        }
                        return Poll::Pending;
                    }
                }
            }
        })
        .await;
        if exit == ExecutionExit::Host(HostExit::Return) {
            self.pending.take();
            stop.clear();
        }
        exit
    }
}
