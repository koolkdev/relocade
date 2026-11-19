//! Arbitration between the CPU, local activities, guest time and the host event loop.

mod guest_time;
mod timing;
pub use guest_time::GuestTimeConfig;
pub(crate) use timing::Timing;

use crate::{
    runner::Activity,
    runtime::CpuStop,
    worker::{yield_to_host, WorkerWake},
    ExecutionInterrupt, Instant, Runner,
};
use futures_channel::mpsc;
use futures_util::{
    future::{select, Either},
    StreamExt,
};
use std::{cell::RefCell, future::Future, pin::pin, rc::Rc, task::Poll, time::Duration};

// A Rust wake announcing a posted object can precede its JavaScript callback.
// Give runnable work this long before returning the worker to its event loop.
const HOST_DELIVERY_GRACE: Duration = Duration::from_millis(1);

pub(crate) struct Scheduler {
    time: Timing,
    inbox: mpsc::UnboundedReceiver<Activity>,
    wake: Rc<WorkerWake<ExecutionInterrupt>>,
}

enum Turn<T> {
    Complete(Option<T>),
    Continue,
    Wait,
}

impl Scheduler {
    pub(crate) fn new(
        time: Timing,
        inbox: mpsc::UnboundedReceiver<Activity>,
        wake: Rc<WorkerWake<ExecutionInterrupt>>,
    ) -> Self {
        Self { time, inbox, wake }
    }

    pub(crate) async fn run<R>(
        self,
        mut runner: Runner,
        stop: impl Future,
        main: impl AsyncFnOnce(Runner) -> R,
    ) -> Option<R> {
        let Self {
            mut time,
            inbox,
            wake,
        } = self;
        let _binding = wake.poll(|cx| time.bind(runner.cpu(), cx.waker()));
        let execution = runner.cpu().execution.clone();
        let running = Rc::new(RefCell::new(None));
        runner.running = Rc::downgrade(&running);
        let mut activities = pin!(inbox.for_each_concurrent(None, |activity| activity));
        let mut main = pin!(main(runner));
        let mut stop = pin!(stop);
        let host = wake.host_turns();
        let mut host_deadline = None;

        loop {
            let turn = wake.poll(|cx| {
                if stop.as_mut().poll(cx).is_ready() {
                    return Turn::Complete(None);
                }
                let _ = activities.as_mut().poll(cx);
                if let Poll::Ready(result) = main.as_mut().poll(cx) {
                    return Turn::Complete(Some(result));
                }
                let _permit = match &execution {
                    Some(member) => match member.poll_enter(cx) {
                        Poll::Ready(permit) => Some(permit),
                        Poll::Pending => return Turn::Wait,
                    },
                    None => None,
                };
                let mut active = running.borrow_mut();
                let boundary = match active.as_ref() {
                    Some(run) => {
                        let mut cpu = run.cpu.borrow_mut();
                        if let Some(exit) = cpu.poll_host_work(cx) {
                            CpuStop::Exit(exit)
                        } else if time.poll_pacing(&cpu, cx).is_pending() {
                            if !cpu.is_blocked() {
                                return Turn::Wait;
                            }
                            CpuStop::Blocked
                        } else {
                            cpu.run_to_boundary(cx)
                        }
                    }
                    None => CpuStop::Blocked,
                };
                // HLT can retire instructions on its way to a blocked boundary.
                time.notify_due();
                match boundary {
                    CpuStop::Exit(exit) => {
                        let run = active.take().expect("only a running CPU can exit");
                        let _ = run.completed.send(exit);
                    }
                    CpuStop::Checkpoint => {}
                    CpuStop::Blocked => {
                        // A woken activity may cancel the timer or install an earlier one.
                        if wake.has_pending_poll() || !time.advance_idle(cx) {
                            return Turn::Wait;
                        }
                    }
                }
                Turn::Continue
            });
            if let Turn::Complete(result) = turn {
                return result;
            }
            if !wake.take_pending_poll() && matches!(turn, Turn::Wait) {
                // Parking already lets the browser deliver its callbacks.
                host_deadline = None;
                wake.wait().await;
            }
            if host.has_pending() {
                let deadline =
                    host_deadline.get_or_insert_with(|| Instant::now() + HOST_DELIVERY_GRACE);
                if Instant::now() >= *deadline {
                    if let Either::Left(_) = select(stop.as_mut(), pin!(yield_to_host())).await {
                        return None;
                    }
                    host_deadline = None;
                }
                // Even an unthrottled CPU must return while a JS callback is outstanding.
                wake.request_checkpoint();
            } else {
                host_deadline = None;
            }
        }
    }
}
