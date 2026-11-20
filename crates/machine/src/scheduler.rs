//! Guest device timing and CPU pacing at execution boundaries.

mod config;
mod guest_time;
mod timing;

pub use config::GuestTimeConfig;
pub use guest_time::GuestTimeDriver;
pub(crate) use timing::Timing;

use crate::{worker::WorkerWake, Cpu, ExecutionInterrupt};
use relocade_x86::ExecutionExit;
use std::task::{ready, Context, Poll};

impl Timing {
    /// Services ready host work before charging guest instructions or waiting for a device.
    pub(crate) fn poll_cpu(
        &mut self,
        cpu: &mut Cpu,
        wake: &WorkerWake<ExecutionInterrupt>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<ExecutionExit>> {
        let _execution = ready!(cpu.poll_enter_execution(cx));
        let boundary = if let Some(exit) = cpu.poll_host_call(cx) {
            Poll::Ready(exit)
        } else if self.poll_pacing(cpu, cx).is_ready() {
            cpu.poll_execution(cx)
        } else if cpu.is_blocked() {
            Poll::Pending
        } else {
            return Poll::Pending;
        };
        self.notify_due();
        match boundary {
            Poll::Ready(ExecutionExit::Interrupted | ExecutionExit::BudgetExhausted) => {
                Poll::Ready(None)
            }
            Poll::Ready(exit) => Poll::Ready(Some(exit)),
            Poll::Pending => {
                // Give ready tasks a turn to cancel or replace their device waits.
                if wake.has_pending_poll() || self.advance_idle(cx) {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        }
    }
}
