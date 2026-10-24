use super::{
    deferred::{CallFrame, Invocation},
    state::PendingCall,
    Call, Cpu, HookResult, Result,
};
use relocade_wasm_runtime::WasmContext;
use relocade_x86::ExecutionExit;
use std::{
    future::poll_fn,
    rc::Rc,
    task::{Context, Poll},
};

impl<T: 'static> Cpu<T> {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host input as needed. Ready operations complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound.
    /// Waits do not replenish the budget. Dropping this waiter preserves pending
    /// services and their locals; a later run resumes the same operation.
    /// Use `cancel_pending_call` when replacing the suspended program.
    /// A zero budget neither enters Wasm nor polls pending work.
    pub async fn run(&mut self, instruction_budget: u64) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self.instruction_target(instruction_budget);
        poll_fn(|cx| self.poll_run(cx, target)).await
    }

    /// Drops queued and suspended operations without undoing their side effects.
    /// The embedding must replace their guest continuation before running again.
    /// Call handles become invalid before their futures are dropped.
    pub fn cancel_pending_call(&mut self) {
        let pending = {
            let mut core = self.core.borrow_mut();
            core.active_call = None;
            let pending = core.backend.state_mut().pending.take();
            pending
        };
        drop(pending);
        while let Some(frame) = self.calls.pop() {
            self.drop_invocation(frame.invocation);
        }
    }

    fn instruction_target(&mut self, instruction_budget: u64) -> u64 {
        self.core
            .borrow_mut()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget)
    }

    fn poll_run(&mut self, cx: &mut Context<'_>, target: u64) -> Poll<Result<ExecutionExit>> {
        loop {
            if self.core.borrow_mut().cpu().instruction_count() >= target {
                return Poll::Ready(Ok(ExecutionExit::BudgetExhausted));
            }
            let pending = self.core.borrow_mut().backend.state_mut().pending.take();
            if let Some(pending) = pending {
                self.start_call(pending);
            }
            if let Some(frame) = self.calls.last_mut() {
                assert_eq!(self.core.borrow().active_call, Some(frame.id));
                let result = match &mut frame.invocation {
                    Invocation::Registered { handler, slot } => {
                        self.handlers[*handler].poll(*slot, cx)
                    }
                    Invocation::Once(future) => future.as_mut().poll(cx),
                };
                let Poll::Ready(result) = result else {
                    return Poll::Pending;
                };
                let frame = self.calls.pop().expect("completed host call");
                self.core.borrow_mut().active_call = None;
                self.drop_invocation(frame.invocation);
                match result {
                    HookResult::Continue => {}
                    HookResult::Exit(ExecutionExit::Deferred) => {
                        assert!(
                            self.core.borrow().backend.state().pending.is_some(),
                            "deferred exit must queue a host call"
                        );
                        continue;
                    }
                    HookResult::Exit(exit) => return Poll::Ready(Ok(exit)),
                }
            }
            let exit = {
                let core = &mut *self.core.borrow_mut();
                core.bindings
                    .set_instruction_target(&mut core.backend, target);
                core.dispatch.run(&mut core.backend)
            };
            if exit != ExecutionExit::Deferred {
                return Poll::Ready(Ok(exit));
            }
            assert!(
                self.core.borrow().backend.state().pending.is_some(),
                "deferred exit must queue a host call"
            );
        }
    }

    fn start_call(&mut self, pending: PendingCall<T>) {
        self.next_call = self
            .next_call
            .checked_add(1)
            .expect("host invocation identity exhausted");
        let id = self.next_call;
        self.core.borrow_mut().active_call = Some(id);
        let call = Call {
            core: Rc::downgrade(&self.core),
            id,
        };
        let invocation = match pending {
            PendingCall::Registered(handler) => {
                let slot = self.handlers[handler].start(call);
                Invocation::Registered { handler, slot }
            }
            PendingCall::Once(factory) => Invocation::Once(factory.start(call)),
        };
        self.calls.push(CallFrame { id, invocation });
    }

    fn drop_invocation(&mut self, invocation: Invocation) {
        if let Invocation::Registered { handler, slot } = invocation {
            self.handlers[handler].clear(slot);
        }
    }
}
