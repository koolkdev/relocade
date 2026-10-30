//! Execution owns IRQ delivery, suspended frames and explicit guest resumption.

use super::{machine::Machine, Call, Cpu, HookResult};
use relocade_x86::{Exception, ExecutionExit, HostExit, Segment};
use std::{
    future::poll_fn,
    task::{Context, Poll, Waker},
};

pub(super) struct Frame<T> {
    call: Call<T>,
    pub address: u32,
}

enum Event {
    Complete(HookResult),
    Interrupt(u8),
}

impl<T: 'static> Cpu<T> {
    fn start_call(&mut self, address: u32) -> Result<Frame<T>, ExecutionExit> {
        let mut machine = self.machine.borrow_mut();
        let service = machine.hooks.get(&address).expect("registered host entry");
        let (handler, resume) = (service.handler.clone(), service.resume);
        {
            let mut context = machine.context();
            let cs = context.cpu.segment(Segment::Cs);
            let offset = resume
                .checked_sub(cs.base)
                .filter(|&offset| offset <= cs.limit)
                .ok_or(ExecutionExit::Exception(Exception::GeneralProtection {
                    error_code: 0,
                }))?;
            context.cpu.set_eip(offset);
        }
        drop(machine);
        Ok(Frame {
            call: Call::start(&self.machine, handler),
            address,
        })
    }

    fn poll_call(&mut self, cx: &mut Context<'_>) -> Poll<Event> {
        let Some(frame) = &mut self.active_call else {
            return Poll::Ready(Event::Complete(HookResult::Continue));
        };
        if let Poll::Ready(result) = frame.call.poll(cx) {
            return Poll::Ready(Event::Complete(result));
        }
        poll_interrupt(&mut self.machine.borrow_mut(), cx).map(Event::Interrupt)
    }

    /// Abandons parked host invocations after a deliberate guest control-flow change.
    /// Their futures and locals are released without restoring IF or CPU state.
    /// The guest must no longer return to the resume entry for these frames.
    /// A pending active invocation, if any, is unaffected.
    pub fn abandon_suspended_calls(&mut self) {
        self.suspended_calls.clear();
    }

    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host calls as needed. Ready calls complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish the budget. Dropping a waiter preserves its pending
    /// call and locals. A later run supplies a replacement budget for continued
    /// execution. A zero budget never enters Wasm or polls pending work.
    ///
    /// IRQs are checked at new quanta, after host calls continue guest dispatch,
    /// and whenever a host future returns Pending, subject to live IF. Interrupting
    /// a host call saves its live position at the service's resume label. Returning
    /// there resumes the matching top frame without restoring any CPU state.
    /// Guest register, flags, stack and memory effects are preserved. A resume
    /// for a different service, or with no parked frame, returns `HostExit::Resume`
    /// to the embedding and leaves the suspended work intact.
    ///
    /// ISR budget/fault exits retain suspended futures. Input readiness alone does
    /// not resume them. Redirecting guest execution away from the resume entry
    /// leaves them parked until explicit [`Self::abandon_suspended_calls`], session
    /// `HostExit::Return`, or CPU destruction. No IRET count or stack equality is used.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        let mut budget = Some(instruction_budget);
        let exit = loop {
            let event = poll_fn(|cx| self.poll_call(cx)).await;
            let exit = match event {
                Event::Complete(HookResult::Continue) => {
                    self.active_call = None;
                    let mut machine = self.machine.borrow_mut();
                    match poll_interrupt(&mut machine, &mut Context::from_waker(Waker::noop())) {
                        Poll::Ready(vector) => machine.interrupt(vector, budget.take()),
                        Poll::Pending => machine.dispatch(budget.take()),
                    }
                }
                Event::Interrupt(vector) => {
                    let mut machine = self.machine.borrow_mut();
                    self.suspended_calls
                        .push(self.active_call.take().expect("interrupted host call"));
                    // The live PC already names this service's resume label.
                    machine.interrupt(vector, budget.take())
                }
                Event::Complete(HookResult::Exit(exit)) => {
                    self.active_call = None;
                    break exit;
                }
            };
            match exit {
                ExecutionExit::Host(HostExit::Call { address }) => match self.start_call(address) {
                    Ok(frame) => self.active_call = Some(frame),
                    Err(exit) => break exit,
                },
                ExecutionExit::Host(HostExit::Resume { address }) => {
                    if !self
                        .suspended_calls
                        .last()
                        .is_some_and(|frame| frame.address == address)
                    {
                        break exit;
                    }
                    self.active_call = self.suspended_calls.pop();
                }
                exit => break exit,
            }
        };
        if exit == ExecutionExit::Host(HostExit::Return) {
            self.abandon_suspended_calls();
        }
        exit
    }
}

fn poll_interrupt<T>(machine: &mut Machine<T>, cx: &mut Context<'_>) -> Poll<u8> {
    if !machine.context().cpu.interrupts_enabled() {
        return Poll::Pending;
    }
    machine.io.poll_interrupt(cx)
}
