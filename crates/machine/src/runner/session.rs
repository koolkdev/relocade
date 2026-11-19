//! Local activities dispatched by the execution worker's ready queue.

use super::schedule::{ExecutionSchedule, Turn};
use crate::{machine::execution::Member, worker::Executor, Cpu, ExecutionInterrupt};
use futures_channel::oneshot;
use futures_util::future::LocalBoxFuture;
use relocade_x86::ExecutionExit;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::pin,
    rc::{Rc, Weak},
    task::{ready, Context, Poll, Waker},
};

const APPLICATION: usize = 0;
const MACHINE: usize = 1;

pub(super) struct Session {
    pub(super) machine: Waker,
    running: RefCell<Option<CpuRun>>,
    activities: RefCell<HashMap<usize, Activity>>,
    next_activity: Cell<usize>,
}

struct Activity {
    future: LocalBoxFuture<'static, ()>,
    started: Option<oneshot::Sender<()>>,
}

struct CpuRun {
    cpu: Rc<RefCell<Cpu>>,
    completed: oneshot::Sender<ExecutionExit>,
}

impl Session {
    pub(super) fn new(executor: &Executor<ExecutionInterrupt>) -> Rc<Self> {
        Rc::new(Self {
            machine: executor.task(MACHINE),
            running: RefCell::new(None),
            activities: RefCell::new(HashMap::new()),
            next_activity: Cell::new(MACHINE + 1),
        })
    }

    pub(super) fn spawn(
        &self,
        executor: &Executor<ExecutionInterrupt>,
        future: LocalBoxFuture<'static, ()>,
        started: Option<oneshot::Sender<()>>,
    ) {
        let id = self.next_activity.get();
        // An escaped waker must never refer to a later activity.
        self.next_activity
            .set(id.checked_add(1).expect("activity id space exhausted"));
        self.activities
            .borrow_mut()
            .insert(id, Activity { future, started });
        executor.task(id).wake();
    }

    fn poll_activity(&self, id: usize, cx: &mut Context<'_>) {
        // An activity can register further activities from inside its poll.
        let Some(mut activity) = self.activities.borrow_mut().remove(&id) else {
            return;
        };
        let result = activity.future.as_mut().poll(cx);
        if let Some(started) = activity.started.take() {
            let _ = started.send(());
        }
        if result.is_pending() {
            self.activities.borrow_mut().insert(id, activity);
        }
    }

    pub(super) fn run_cpu(
        self: &Rc<Self>,
        cpu: Rc<RefCell<Cpu>>,
    ) -> (CancelRun, oneshot::Receiver<ExecutionExit>) {
        let (completed, exit) = oneshot::channel();
        self.running.replace(Some(CpuRun { cpu, completed }));
        self.machine.wake_by_ref();
        (CancelRun(Rc::downgrade(self)), exit)
    }

    pub(super) async fn drive<R>(
        &self,
        executor: &Executor<ExecutionInterrupt>,
        stop: impl Future,
        application: impl Future<Output = R>,
        mut schedule: ExecutionSchedule,
        execution: Option<Member>,
    ) -> Option<R> {
        let mut application = pin!(application);
        executor.task(APPLICATION).wake();
        self.machine.wake_by_ref();
        executor
            .drive(stop, |id, cx| {
                match id {
                    APPLICATION => return application.as_mut().poll(cx),
                    MACHINE => {
                        let mut active = self.running.borrow_mut();
                        let turn = {
                            let mut cpu = active.as_ref().map(|run| run.cpu.borrow_mut());
                            ready!(schedule.poll_turn(
                                cpu.as_deref_mut(),
                                execution.as_ref(),
                                || executor.has_ready(),
                                cx,
                            ))
                        };
                        match turn {
                            Turn::Continue => self.machine.wake_by_ref(),
                            Turn::Exit(exit) => {
                                let run = active.take().expect("a CPU was running");
                                let _ = run.completed.send(exit);
                                // Device deadlines can outlive the foreground.
                                self.machine.wake_by_ref();
                            }
                        }
                    }
                    id => self.poll_activity(id, cx),
                }
                Poll::Pending
            })
            .await
    }
}

pub(super) struct CancelRun(Weak<Session>);

impl Drop for CancelRun {
    fn drop(&mut self) {
        if let Some(session) = self.0.upgrade() {
            if session.running.take().is_some() {
                session.machine.wake_by_ref();
            }
        }
    }
}
