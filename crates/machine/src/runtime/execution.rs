use super::{calls::PendingCall, Cpu, CpuContext, HookResult};
use relocade_x86::ExecutionExit;
use std::{
    future::poll_fn,
    rc::Rc,
    task::{Context, Poll},
};

impl<T: 'static> Cpu<T> {
    /// Runs until the instruction budget is consumed or the guest exits,
    /// awaiting host services as needed. Ready services complete in the same poll.
    /// Whole blocks can exceed the budget; straight-line code and REP have no
    /// fixed overshoot or elapsed-time bound. The target saturates at u64::MAX.
    ///
    /// Waits do not replenish this run's budget. Dropping its waiter preserves
    /// pending services and their locals. A subsequent run resumes the same service
    /// with a new budget. Zero neither enters Wasm nor polls pending work.
    pub async fn run(&mut self, instruction_budget: u64) -> ExecutionExit {
        if instruction_budget == 0 {
            return ExecutionExit::BudgetExhausted;
        }
        let target = self
            .context()
            .cpu()
            .instruction_count()
            .saturating_add(instruction_budget);
        self.core
            .bindings
            .set_instruction_target(&mut *self.core.backend.borrow_mut(), target);
        poll_fn(|cx| self.poll_run(cx)).await
    }

    /// Drops a suspended service without undoing its effects or changing CPU state.
    /// Replace its guest continuation before running again. All copies of the
    /// service's context are invalidated before its future is dropped.
    pub fn cancel_pending_call(&mut self) {
        self.core.active_call.set(None);
        self.pending = None;
    }

    fn poll_run(&mut self, cx: &mut Context<'_>) -> Poll<ExecutionExit> {
        loop {
            if let Some(pending) = &self.pending {
                assert_eq!(self.core.active_call.get(), Some(pending.id));
                let result = pending.handler.borrow_mut().poll(pending.slot, cx);
                let Poll::Ready(result) = result else {
                    return Poll::Pending;
                };
                self.core.active_call.set(None);
                self.pending = None;
                match result {
                    HookResult::Continue => {}
                    HookResult::Exit(exit) => return Poll::Ready(exit),
                }
            }
            // No backend or environment borrow survives a service poll.
            self.core.running.set(true);
            let exit = self.dispatch.run(&mut self.core.backend.borrow_mut());
            self.core.running.set(false);
            let ExecutionExit::HostCall { address } = exit else {
                return Poll::Ready(exit);
            };
            self.next_call = self
                .next_call
                .checked_add(1)
                .expect("host invocation identity exhausted");
            self.core.active_call.set(Some(self.next_call));
            let handler = self.hooks.get(&address).expect("registered host entry");
            let started = {
                let mut handler = handler.borrow_mut();
                let started = handler.start(CpuContext::new(&self.core, Some(self.next_call)), cx);
                if started.result.is_ready() {
                    // Revoke access before dropping the completed future's locals.
                    self.core.active_call.set(None);
                    handler.clear(started.slot);
                }
                started
            };
            match started.result {
                Poll::Ready(HookResult::Continue) => {}
                Poll::Ready(HookResult::Exit(exit)) => return Poll::Ready(exit),
                Poll::Pending => {
                    self.pending = Some(PendingCall {
                        id: self.next_call,
                        handler: Rc::clone(handler),
                        slot: started.slot,
                    });
                    return Poll::Pending;
                }
            }
        }
    }
}
