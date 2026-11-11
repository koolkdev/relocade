//! Arbitrates guest calls and device IRQs, suspending host work after entry.

use super::{Cpu, GuestCall, SuspendedCall};
use relocade_x86::{CpuMode, ExecutionExit};
use std::task::{Context, Poll};

impl Cpu {
    /// Enters a host-requested guest call or eligible device IRQ at a stopped boundary.
    /// Both park the active handler until execution reaches its resume entry.
    pub(super) fn try_enter_guest(&mut self, cx: &mut Context<'_>) -> Result<bool, ExecutionExit> {
        if let Some(exit) = self.exit.0.get() {
            return Err(exit);
        }
        let invocation = self.guest_calls.take();
        let entry = if let Some(call) = &invocation {
            call.target.enter(&mut self.instance)
        } else {
            let Poll::Ready(vector) = self.poll_irq(cx)? else {
                return Ok(false);
            };
            // Interrupt recognition ends HLT even if entering the ISR faults.
            self.halted = false;
            GuestCall::Interrupt(vector).enter(&mut self.instance)
        };
        // Frame writes and IVT reads may also request an exit. Observe it before
        // parking a call or executing the ISR, even when IRQ entry faults.
        if let Some(exit) = self.exit.0.get() {
            return Err(exit);
        }
        if let Err(exit) = entry {
            // An entry fault leaves the caller waiting. Retry from that same
            // boundary when the embedding resumes execution.
            if let Some(invocation) = invocation {
                self.guest_calls.put(invocation);
            }
            return Err(exit);
        }
        if let Some(call) = self.pending.take() {
            self.suspended.push(SuspendedCall {
                call,
                returned: invocation.map(|call| call.returned),
            });
        }
        Ok(true)
    }

    fn poll_irq(&self, cx: &mut Context<'_>) -> Result<Poll<u8>, ExecutionExit> {
        let cpu = &self.instance.bindings.cpu;
        if self.instance.io.is_empty() || cpu.mode() != CpuMode::Real || !cpu.interrupts_enabled() {
            return Ok(Poll::Pending);
        }
        let vector = self.instance.io.poll_interrupt(cx);
        // Device callbacks may request an exit even without an eligible IRQ.
        if let Some(exit) = self.exit.0.get() {
            return Err(exit);
        }
        Ok(vector)
    }
}
