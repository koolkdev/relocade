//! CPU allowances and idle advancement on the instruction clock.

use crate::{Cpu, InstructionTimeline};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
    time::Duration,
};

pub(super) enum RunProgress {
    Yield,
    Exit(ExecutionExit),
}

pub(super) struct InstructionSchedule {
    pub(super) timeline: InstructionTimeline,
    quantum: Duration,
}

impl InstructionSchedule {
    pub(super) fn new(timeline: InstructionTimeline, quantum: Duration) -> Self {
        assert!(
            !quantum.is_zero(),
            "instruction scheduling needs a positive quantum"
        );
        Self { timeline, quantum }
    }

    pub(super) fn poll_cpu(&self, cpu: &mut Cpu, cx: &mut Context<'_>) -> Poll<RunProgress> {
        self.timeline.watch_deadlines(cx);
        let budget = self.timeline.instruction_budget(self.quantum);
        if budget == 0 {
            self.timeline.notify_due();
            return Poll::Ready(RunProgress::Yield);
        }
        let result = {
            // The CPU owns suspended services. Replacing this run waiter keeps
            // them alive while recalculating the allowance from current time.
            let mut run = pin!(cpu.run_with_budget(budget));
            run.as_mut().poll(cx)
        };
        match result {
            Poll::Ready(ExecutionExit::BudgetExhausted | ExecutionExit::Interrupted) => {
                Poll::Ready(RunProgress::Yield)
            }
            Poll::Ready(exit) => Poll::Ready(RunProgress::Exit(exit)),
            Poll::Pending => {
                // A device may have been reprogrammed during this poll. Let its
                // activity replace the old deadline before advancing idle time.
                if cpu.interrupt_handle().is_requested() {
                    return Poll::Ready(RunProgress::Yield);
                }
                if let Some(deadline) = self.timeline.next_deadline() {
                    self.timeline.advance_to(deadline);
                    Poll::Ready(RunProgress::Yield)
                } else {
                    // CPU service and IRQ polls already registered their wakers.
                    Poll::Pending
                }
            }
        }
    }

    pub(super) fn enter(&self) -> RunningSchedule<'_> {
        RunningSchedule(&self.timeline)
    }
}

pub(super) struct RunningSchedule<'a>(&'a InstructionTimeline);

impl Drop for RunningSchedule<'_> {
    fn drop(&mut self) {
        self.0.unwatch_deadlines();
    }
}
