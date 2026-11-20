//! One clock owner for the lifetime of a live PC. A foreground request lends
//! the CPU until shutdown, cancellation, or a guest exit, whichever comes first.

use super::schedule::ExecutionSchedule;
use crate::{worker::Executor, Cpu, ExecutionInterrupt};
use futures_channel::oneshot;
use futures_util::future::LocalBoxFuture;
use relocade_x86::ExecutionExit;
use std::{
    cell::{OnceCell, RefCell},
    future::poll_fn,
    rc::Rc,
    task::{Context, Poll, Waker},
};

/// Clears a retained runner's live session on return, stop, or cancellation.
pub(super) struct SessionScope(pub(super) Rc<RefCell<Option<Session>>>);

impl Drop for SessionScope {
    fn drop(&mut self) {
        self.0.borrow_mut().take();
    }
}

pub(super) struct Session {
    driver: LocalBoxFuture<'static, ()>,
    pub(super) foreground: Rc<Foreground>,
}

impl Session {
    pub(super) fn new(
        cpu: &Cpu,
        schedule: Rc<ExecutionSchedule>,
        executor: Rc<Executor<ExecutionInterrupt>>,
    ) -> Self {
        let state = cpu.cpu_state();
        let target = cpu.instruction_target();
        let execution = cpu.execution.clone();
        let foreground = Rc::<Foreground>::default();
        let requests = foreground.clone();
        let driver = Box::pin(async move {
            let changed = poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
            let changed = requests.driver.get_or_init(|| changed);
            let _clock = schedule.bind(state, target, changed);
            loop {
                poll_fn(|cx| {
                    let mut request = requests.run.borrow_mut();
                    let active = request.as_mut().filter(|run| run.completion.is_some());
                    let result = match active {
                        Some(run) => {
                            let result =
                                schedule.poll_turn(Some(&mut run.cpu), execution.as_ref(), cx);
                            if let Poll::Ready(Some(exit)) = result {
                                let _ = run.completion.take().unwrap().send(exit);
                            }
                            result
                        }
                        None => schedule.poll_turn(None, execution.as_ref(), cx),
                    };
                    result.map(|_| ())
                })
                .await;
                executor.yield_now().await;
            }
        });
        Self { driver, foreground }
    }

    pub(super) fn poll(&mut self, cx: &mut Context<'_>) {
        let _ = self.driver.as_mut().poll(cx);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A waiter can live on another local task. Release its completion
        // sender even if that waiter still holds a reference to Foreground.
        if let Some(run) = self.foreground.run.borrow_mut().as_mut() {
            run.completion.take();
        }
    }
}

struct Run {
    cpu: Cpu,
    completion: Option<oneshot::Sender<ExecutionExit>>,
}

#[derive(Default)]
pub(super) struct Foreground {
    run: RefCell<Option<Run>>,
    driver: OnceCell<Waker>,
}

impl Foreground {
    pub(super) async fn run(&self, cpu: &mut Option<Cpu>) -> ExecutionExit {
        let (completion, result) = oneshot::channel();
        self.run.replace(Some(Run {
            cpu: cpu.take().expect("the foreground owns its stopped CPU"),
            completion: Some(completion),
        }));
        if let Some(driver) = self.driver.get() {
            driver.wake_by_ref();
        }
        let _loan = CpuLoan {
            cpu,
            foreground: self,
        };
        result.await.unwrap_or(ExecutionExit::Interrupted)
    }
}

// Cancellation returns the CPU synchronously before the caller can configure
// it again. Pending host services live inside Cpu and can resume on its next run.
struct CpuLoan<'a> {
    cpu: &'a mut Option<Cpu>,
    foreground: &'a Foreground,
}

impl Drop for CpuLoan<'_> {
    fn drop(&mut self) {
        let run = self.foreground.run.take().unwrap();
        *self.cpu = Some(run.cpu);
        if run.completion.is_some() {
            if let Some(driver) = self.foreground.driver.get() {
                driver.wake_by_ref();
            }
        }
    }
}
