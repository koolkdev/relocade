//! CPU execution and registered device activities on one owner.

mod guest_time;
mod schedule;
pub use guest_time::GuestTimeConfig;
mod worker;

use schedule::{ExecutionSchedule, Turn};
pub use worker::ExecutionWorker;

use crate::worker::{Executor, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
/// Gives other ready Rust activities a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::{
    future::{join_all, LocalBoxFuture},
    StreamExt,
};
use relocade_x86::ExecutionExit;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::{Rc, Weak},
    task::{ready, Poll},
    time::Duration,
};

type Activity = LocalBoxFuture<'static, ()>;

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    clock: Clock,
    running: Weak<RefCell<Option<CpuRun>>>,
    activities: mpsc::UnboundedSender<Activity>,
    session: Option<(ExecutionSchedule, mpsc::UnboundedReceiver<Activity>)>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Rc<RefCell<Cpu>>,
}

/// The root can enter this CPU only while the foreground caller awaits its exit.
struct CpuRun {
    cpu: Rc<RefCell<Cpu>>,
    completed: oneshot::Sender<ExecutionExit>,
}

struct CancelRun<'a>(&'a Runner);

impl Drop for CancelRun<'_> {
    fn drop(&mut self) {
        if let Some(running) = self.0.running.upgrade() {
            if running.take().is_some() {
                self.0.executor.request_poll();
            }
        }
    }
}

impl Runner {
    /// Constructs the local CPU backend and binds its wake endpoint.
    /// The supplied clock must progress independently of this session. Use
    /// [`Self::with_guest_time`] for a clock advanced by CPU execution.
    /// Normally called by [`ExecutionWorker`]. The endpoint must come from a
    /// connection created for this CPU; retain its host until the session ends.
    pub async fn new(
        clock: Clock,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        Self::with_schedule(
            ExecutionSchedule::from_clock(clock.clone()),
            cpu,
            endpoint,
            Some(clock),
        )
        .await
    }

    async fn with_schedule(
        schedule: ExecutionSchedule,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
        host_clock: Option<Clock>,
    ) -> Result<Self> {
        let (activities, pending) = mpsc::unbounded();
        let mut executor = Executor::new(endpoint.bind(cpu.interrupt_handle()));
        if let Some(clock) = host_clock {
            executor =
                executor.with_host_timer(Duration::from_millis(1), move |delay| clock.sleep(delay));
        }
        let executor = Rc::new(executor);
        let (cpu, compiler) = cpu.into_session(IoBus::default()).await?;
        let compiler = compiler
            .map(|compiler| compiler.connect(&executor))
            .transpose()?;
        let cpu = cpu.with_compiler(compiler);
        Ok(Self {
            clock: schedule.clock().clone(),
            running: Weak::new(),
            activities,
            session: Some((schedule, pending)),
            executor,
            workers: Rc::new(RefCell::new(Vec::new())),
            cpu: Rc::new(RefCell::new(cpu)),
        })
    }

    /// The shared memory established by the machine before execution started.
    pub fn memory(&self) -> Memory {
        self.cpu.borrow().memory()
    }

    /// The session's device bus for setup and synchronous host I/O.
    pub fn io(&self) -> IoBus {
        self.cpu.borrow().io()
    }

    /// Borrows this runner's stopped CPU for setup and low-level execution.
    /// The borrow must end before calling [`Self::run_cpu`].
    pub fn cpu(&mut self) -> &mut Cpu {
        Rc::get_mut(&mut self.cpu)
            .expect("the stopped CPU belongs to its runner")
            .get_mut()
    }

    /// Takes this CPU's compiler connection once.
    /// Returns `None` if no service was supplied, the connection closed, or it was taken.
    /// The session receives ordered results through this single owner.
    /// Take it during setup to manage requests and installation explicitly.
    /// Panics if automatic compilation requests are still outstanding.
    pub fn take_compiler(&mut self) -> Option<crate::CompilerClient> {
        self.cpu().take_compiler()
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Post this resource to another worker and retain its `bind()` guard there
    /// before sending ordinary Rust notifications to this execution session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.executor.peer()
    }

    /// Starts a reusable calculation thread owned by this execution session.
    /// Returns after the thread binds the session's wake peer. Await during setup
    /// so browser startup can finish before synchronous guest execution begins.
    /// The session retains the worker if this startup wait is cancelled.
    /// Each submitted closure owns its inputs and returns its result asynchronously.
    /// The submitting activity can await a result while guest execution continues.
    /// Session shutdown closes admission and drains accepted work before joining.
    pub async fn spawn_job_thread(&self) -> JobExecutor {
        let (queue, jobs) = JobQueue::new();
        let (started, ready) = oneshot::channel();
        let worker = Worker::spawn_with(self.wake_peer(), move |peer, stopped| async move {
            let _binding = peer.bind();
            let _ = started.send(());
            queue.run(stopped).await;
        });
        self.workers.borrow_mut().push(Box::pin(async move {
            let _ = worker.stop().await.expect("session job thread cleanup");
        }));
        ready.await.expect("session job thread startup");
        jobs
    }

    /// Connects a device's mappings and starts its activity on this session.
    /// Await this during setup inside [`Self::run`]. Configuration finishes and
    /// releases the runtime context before activity starts. On return, activity
    /// has completed its first poll with the device waker installed.
    /// Once queued, the connected device and its activity belong to the session
    /// even if this wait is cancelled.
    pub async fn connect<D: Device + 'static>(
        &self,
        device: Rc<RefCell<D>>,
        configure: impl FnOnce(&mut DeviceConnection<'_>),
    ) {
        self.io().connect(device.clone(), configure);
        self.spawn_activity(D::run(device)).await;
    }

    /// Starts a local activity with the same lifetime and first-poll guarantee
    /// as a connected device. Use for services without bus mappings.
    pub async fn spawn_activity(&self, activity: impl Future<Output = ()> + 'static) {
        let (started, ready) = oneshot::channel();
        let activity = async move {
            let mut run = pin!(activity);
            let mut started = Some(started);
            poll_fn(|cx| {
                let result = run.as_mut().poll(cx);
                if let Some(started) = started.take() {
                    let _ = started.send(());
                }
                result
            })
            .await;
        };
        self.activities
            .unbounded_send(Box::pin(activity))
            .unwrap_or_else(|_| panic!("runner must own its activities"));
        ready.await.expect("runner must poll registered activities");
    }

    /// Runs the main activity and devices for one execution lifetime.
    /// Devices remain active while the main activity awaits input or another
    /// process. Completion or cancellation drops every activity on this owner,
    /// then drains calculations and joins their threads.
    /// The stop future is registered before application code runs. It returns
    /// `None` on cancellation; ordinary main completion returns `Some(result)`.
    pub async fn run<R>(
        mut self,
        stop: impl Future,
        main: impl AsyncFnOnce(Self) -> R,
    ) -> Option<R> {
        let (schedule, pending) = self.session.take().expect("new runner session");
        let devices = pending.for_each_concurrent(None, |activity| activity);
        let executor = self.executor.clone();
        let _activities = self.activities.clone();
        let workers = self.workers.clone();
        let result = executor
            .run_until(stop, async {
                let mut schedule = schedule;
                let wake = poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
                let running = Rc::new(RefCell::new(None::<CpuRun>));
                self.running = Rc::downgrade(&running);
                let execution = self.cpu.borrow().execution.clone();
                let interrupt = self.cpu.borrow().interrupt_handle();
                let _binding = schedule.bind(&self.cpu.borrow(), &wake);
                let mut devices = pin!(devices);
                let mut application = pin!(main(self));
                poll_fn(|cx| {
                    let was_running = running.borrow().is_some();
                    let _ = devices.as_mut().poll(cx);
                    if let Poll::Ready(result) = application.as_mut().poll(cx) {
                        return Poll::Ready(result);
                    }
                    let mut active = running.borrow_mut();
                    // A newly submitted run gives the executor a boundary to
                    // acknowledge the wake that made its caller runnable.
                    if !was_running && active.is_some() && interrupt.is_requested() {
                        return Poll::Pending;
                    }
                    let turn = {
                        let mut cpu = active.as_ref().map(|run| run.cpu.borrow_mut());
                        ready!(schedule.poll_turn(
                            cpu.as_deref_mut(),
                            execution.as_ref(),
                            &interrupt,
                            cx
                        ))
                    };
                    match turn {
                        Turn::Continue => executor.request_poll(),
                        Turn::UnpacedIdle => executor.request_host_turn(),
                        Turn::Exit(exit) => {
                            let run = active.take().expect("only a running CPU can exit");
                            let _ = run.completed.send(exit);
                            executor.request_poll();
                        }
                    }
                    Poll::Pending
                })
                .await
            })
            .await;
        let cleanup = std::mem::take(&mut *workers.borrow_mut());
        join_all(cleanup).await;
        result
    }

    /// Requests guest execution and awaits its exit inside [`Self::run`].
    /// Dropping the wait stops execution and preserves pending CPU services.
    /// Ending the session interrupts an outstanding wait.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let (completed, exit) = oneshot::channel();
        let Some(running) = self.running.upgrade() else {
            return ExecutionExit::Interrupted;
        };
        running.replace(Some(CpuRun {
            cpu: self.cpu.clone(),
            completed,
        }));
        self.executor.request_poll();
        drop(running);
        let _registration = CancelRun(self);
        exit.await.unwrap_or(ExecutionExit::Interrupted)
    }

    /// Runs on emulated guest time with optional host pacing for active and idle
    /// execution. Device deadlines subdivide each execution slice. Instruction targets
    /// remain soft checkpoint limits, so a slice can overshoot.
    /// The session advances guest deadlines even while its CPU is stopped.
    pub async fn with_guest_time(
        config: GuestTimeConfig,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        let host_clock = config.host_clock.clone();
        Self::with_schedule(
            ExecutionSchedule::with_guest_time(config),
            cpu,
            endpoint,
            host_clock,
        )
        .await
    }
}
