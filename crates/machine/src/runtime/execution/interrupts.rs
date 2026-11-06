//! Interrupt calls and device IRQs share entry and suspended-call handling.

use super::{Cpu, SuspendedCall};
use futures_channel::oneshot;
use relocade_x86::{ExecutionExit, ExecutionProfile, HostExit};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll},
};

#[derive(Clone, Default)]
pub(in crate::runtime) struct InterruptCalls(Rc<RefCell<Option<Request>>>);

struct Request {
    vector: u8,
    returned: oneshot::Sender<()>,
}

impl InterruptCalls {
    pub async fn call(&self, vector: u8) {
        let (returned, completion) = oneshot::channel();
        self.put(Request { vector, returned });
        completion
            .await
            .expect("interrupt caller retained until return");
    }

    fn put(&self, request: Request) {
        let mut pending = self.0.borrow_mut();
        assert!(
            pending
                .as_ref()
                .is_none_or(|call| call.returned.is_canceled()),
            "only one interrupt may be requested by a host handler"
        );
        *pending = Some(request);
    }

    fn take(&self) -> Option<Request> {
        // Selecting another future can cancel an invocation before the driver
        // sees it. Such a request must not change the guest's CPU or stack.
        self.0
            .borrow_mut()
            .take()
            .filter(|call| !call.returned.is_canceled())
    }
}

impl Cpu {
    /// Enters a Rust interrupt call or an eligible device IRQ at a stopped boundary.
    /// Both park the active handler until execution reaches its resume entry.
    pub(super) fn try_deliver_interrupt(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Result<bool, ExecutionExit> {
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let invocation = self.interrupt_calls.take();
        let interrupt = match &invocation {
            Some(call) => Poll::Ready(call.vector),
            None => self.poll_irq(cx),
        };
        // Device callbacks can request an exit, including when no IRQ is ready.
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        let Poll::Ready(vector) = interrupt else {
            return Ok(false);
        };
        let entered = self
            .machine
            .dispatch
            .enter_interrupt(&mut self.machine.backend, vector);
        // Frame writes and IVT reads may also request an exit. Observe it before
        // parking a call or executing the ISR, even when IRQ entry faults.
        if self.exit.0.get() {
            return Err(ExecutionExit::Host(HostExit::Return));
        }
        if entered != ExecutionExit::InterruptEntered {
            // An entry fault leaves the caller waiting. Retry from that same
            // boundary when the embedding resumes execution.
            if let Some(invocation) = invocation {
                self.interrupt_calls.put(invocation);
            }
            return Err(entered);
        }
        if let Some(call) = self.pending.take() {
            self.suspended.push(SuspendedCall {
                call,
                returned: invocation.map(|call| call.returned),
            });
        }
        Ok(true)
    }

    fn poll_irq(&self, cx: &mut Context<'_>) -> Poll<u8> {
        let cpu = &self.machine.bindings.cpu;
        if self.machine.io.is_empty()
            || cpu.execution_target().profile != ExecutionProfile::Real16
            || !cpu.interrupts_enabled()
        {
            return Poll::Pending;
        }
        self.machine.io.poll_interrupt(cx)
    }
}
