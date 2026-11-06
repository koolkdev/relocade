//! Resumes or abandons host calls parked during guest interrupt execution.

use super::{Call, Cpu};
use futures_channel::oneshot;
use relocade_x86::{ExecutionExit, HostExit};
use std::task::Context;

pub(in crate::runtime) struct SuspendedCall {
    // Drop the future before its completion sender when abandoning a callback.
    pub call: Call,
    pub returned: Option<oneshot::Sender<()>>,
}

impl Cpu {
    pub(super) fn resume_call(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Result<Option<Call>, ExecutionExit> {
        let call = self
            .suspended
            .pop()
            .ok_or(ExecutionExit::Host(HostExit::Resume))?;
        if let Some(returned) = call.returned {
            let _ = returned.send(());
        }
        Ok(call.call.poll(cx))
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
