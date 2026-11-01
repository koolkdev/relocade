//! Parks host computation while guest ISR code owns execution.

use super::{Call, Cpu};
use relocade_x86::{ExecutionExit, HostExit};
use std::task::{Context, Poll};

impl Cpu {
    pub(super) fn suspend_for_interrupt(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Result<bool, ExecutionExit> {
        let machine = &mut self.machine;
        let interrupt = machine.poll_interrupt(cx);
        if self.exit.0.get() {
            return Ok(false);
        }
        let Poll::Ready(vector) = interrupt else {
            return Ok(false);
        };
        let entered = machine.enter_interrupt(vector);
        if self.exit.0.get() {
            return Ok(false);
        }
        entered?;
        self.suspended
            .push(self.pending.take().expect("interrupted service"));
        Ok(true)
    }

    pub(super) fn resume_call(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Result<Option<Call>, ExecutionExit> {
        let call = self
            .suspended
            .pop()
            .ok_or(ExecutionExit::Host(HostExit::Resume))?;
        Ok(call.poll(cx))
    }

    /// Drops parked calls innermost first after guest code redirects execution
    /// away from their resume entries. Leaves an active pending call and live CPU
    /// state untouched. The embedding must ensure abandoned resume entries will
    /// no longer return to those calls. Execution return and CPU teardown also
    /// release parked work; dropping a run waiter does not.
    pub fn abandon_suspended_calls(&mut self) {
        while self.suspended.pop().is_some() {}
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        self.pending.take();
        self.abandon_suspended_calls();
    }
}
