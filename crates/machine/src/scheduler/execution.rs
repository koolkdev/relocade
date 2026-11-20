//! Drives the CPU or advances idle device time while the foreground waits.

use super::Runner;
use crate::runtime::CpuStop;
use futures_util::future::{select, Either};
use relocade_x86::ExecutionExit;
use std::{
    convert::Infallible,
    future::{pending, poll_fn, Future},
    pin::pin,
    task::{ready, Context, Poll},
};

impl Runner {
    /// Waits for foreground work while devices can advance to their next guest deadline.
    /// Guest instructions remain stopped for the duration of the wait.
    pub async fn wait_for<F: Future>(&mut self, work: F) -> F::Output {
        let mut work = pin!(work);
        poll_fn(|cx| {
            if let Poll::Ready(result) = work.as_mut().poll(cx) {
                return Poll::Ready(result);
            }
            let _permit = ready!(self.cpu.poll_enter(cx));
            self.poll_idle(cx);
            Poll::Pending
        })
        .await
    }

    /// Executes guest instructions until exit. Dropping the future immediately returns
    /// the CPU to its caller with any active or parked BIOS calls intact.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        match self.run_cpu_until(pending::<Infallible>()).await {
            Either::Left(exit) => exit,
            Either::Right(never) => match never {},
        }
    }

    /// Runs the CPU while polling foreground work before each guest entry. Returns
    /// the CPU exit on the left or the completed foreground result on the right.
    pub async fn run_cpu_until<F: Future>(&mut self, work: F) -> Either<ExecutionExit, F::Output> {
        let (exit, devices_due) = match select(pin!(work), pin!(self.execute())).await {
            Either::Left((result, _)) => return Either::Right(result),
            Either::Right((boundary, _)) => boundary,
        };
        if devices_due {
            super::yield_now().await;
        }
        Either::Left(exit)
    }

    /// Run through checkpoints until exit. The caller gives any devices due at
    /// the final instruction a turn before returning the committed exit.
    async fn execute(&mut self) -> (ExecutionExit, bool) {
        let run = self.cpu.listen_for_run();
        poll_fn(|cx| {
            let _permit = ready!(run.poll_enter(&mut self.cpu, cx));
            let boundary = if let Some(exit) = self.cpu.poll_host_call(cx) {
                CpuStop::Exit(exit)
            } else if self.time.poll_pacing(&self.cpu, cx).is_ready() {
                self.cpu.run_to_boundary(cx)
            } else if self.cpu.is_blocked() {
                CpuStop::Blocked
            } else {
                return Poll::Pending;
            };
            // HLT and the final guest instruction can both make a device due.
            let due = self.time.notify_due();
            match boundary {
                CpuStop::Exit(exit) => return Poll::Ready((exit, due)),
                CpuStop::Checkpoint => self.wake.repoll(cx),
                CpuStop::Blocked => self.poll_idle(cx),
            }
            Poll::Pending
        })
        .await
    }

    fn poll_idle(&mut self, cx: &mut Context<'_>) {
        // Ready tasks can cancel a timer or replace it with an earlier one.
        if self.wake.has_pending_poll() || self.time.advance_idle(cx) {
            self.wake.repoll(cx);
        }
    }
}
