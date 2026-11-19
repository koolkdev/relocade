//! CPU execution and registered device activities on one owner.

mod guest_time;
mod hardware;
mod schedule;
pub use guest_time::GuestTimeConfig;
mod worker;

pub use hardware::Alarm;
use hardware::Hardware;
use schedule::ExecutionSchedule;
pub use worker::ExecutionWorker;

use crate::runtime::CpuStop;
use crate::worker::{Executor, HostTurns, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
use futures_lite::future::poll_once;
/// Gives other ready Rust activities a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::{
    future::{join_all, LocalBoxFuture},
    Stream, StreamExt,
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

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    clock: Clock,
    hardware: Weak<Hardware>,
    activities: mpsc::UnboundedSender<Activity>,
    session: Option<(ExecutionSchedule, mpsc::UnboundedReceiver<Activity>)>,
    running: Weak<RefCell<Option<CpuRun>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Rc<RefCell<Cpu>>,
}

type Activity = LocalBoxFuture<'static, ()>;

struct CpuRun {
    cpu: Rc<RefCell<Cpu>>,
    completed: oneshot::Sender<ExecutionExit>,
}

// Cancelling the foreground wait returns exclusive access to Runner::cpu immediately.
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
        Self::with_schedule(ExecutionSchedule::from_clock(clock), cpu, endpoint).await
    }

    async fn with_schedule(
        schedule: ExecutionSchedule,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        let executor = Rc::new(
            Executor::new(endpoint.bind(cpu.interrupt_handle()))
                .with_cooperative_host_grace(Duration::from_millis(1)),
        );
        let (cpu, compiler) = cpu.into_session(IoBus::default()).await?;
        let (activities, inbox) = mpsc::unbounded();
        let compiler = compiler
            .map(|endpoint| {
                let (client, replies) = endpoint.connect(&executor)?;
                activities
                    .unbounded_send(replies)
                    .expect("new session activity inbox");
                Ok::<_, crate::Error>(client)
            })
            .transpose()?;
        let cpu = cpu
            .with_compiler(compiler)
            .with_host_turns(executor.host_turns());
        Ok(Self {
            clock: schedule.clock().clone(),
            hardware: Weak::new(),
            activities,
            session: Some((schedule, inbox)),
            running: Weak::new(),
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

    /// Registers a disarmed hardware alarm on the session's clock.
    pub fn alarm(&self, fire: impl FnMut() + 'static) -> Alarm {
        self.alarm_on(&self.clock, fire)
    }

    /// Registers a device that was configured with its own clock. The callback
    /// still runs on this session, after guest execution returns to the worker.
    pub fn alarm_on(&self, clock: &Clock, fire: impl FnMut() + 'static) -> Alarm {
        self.hardware
            .upgrade()
            .expect("alarm needs a running session")
            .alarm(clock.clone(), fire)
    }

    /// Delivers at most one queued item from this stream per worker turn, before
    /// due alarms. Sources are visited in registration order. The session owns
    /// the stream and callback until the stream closes or execution ends.
    pub fn on_input<T>(
        &self,
        source: impl Stream<Item = T> + 'static,
        receive: impl FnMut(T) + 'static,
    ) {
        self.hardware
            .upgrade()
            .expect("input needs a running session")
            .on_input(source, receive);
    }

    /// Post this resource to another worker and retain its `bind()` guard there
    /// before sending ordinary Rust notifications to this execution session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.executor.peer()
    }

    /// Declares an operation whose completion needs this worker's host event loop.
    /// Clone before awaiting a browser promise alongside runnable guest work.
    pub fn host_turns(&self) -> HostTurns {
        self.executor.host_turns()
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
        // A browser worker starts through a callback on its creator.
        let started = if cfg!(target_arch = "wasm32") {
            self.host_turns().wait(ready).await
        } else {
            ready.await
        };
        started.expect("session job thread startup");
        jobs
    }

    /// Connects a device's mappings and attaches it to this session.
    /// Await this during setup inside [`Self::run`]. Configuration finishes and
    /// releases the runtime context before the device installs alarms or starts
    /// an activity. The default activity completes its first poll before this
    /// returns. Once queued, it belongs to the session even if the wait is cancelled.
    pub async fn connect<D: Device + 'static>(
        &self,
        device: Rc<RefCell<D>>,
        configure: impl FnOnce(&mut DeviceConnection<'_>),
    ) {
        self.io().connect(device.clone(), configure);
        D::attach(&device, self).await;
    }

    /// Starts a local activity with the same lifetime and first-poll guarantee
    /// as a connected device. Use for services without bus mappings.
    pub async fn spawn_activity(&self, activity: impl Future<Output = ()> + 'static) {
        let (started, ready) = oneshot::channel();
        let activity = async move {
            let mut run = pin!(activity);
            let completed = poll_once(run.as_mut()).await.is_some();
            let _ = started.send(());
            if !completed {
                run.await;
            }
        };
        self.activities
            .unbounded_send(Box::pin(activity))
            .expect("running session");
        ready
            .await
            .expect("session must poll registered activities");
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
        let (mut schedule, inbox) = self.session.take().expect("new execution session");
        let executor = self.executor.clone();
        let workers = self.workers.clone();
        let result = executor
            .run_until(stop, async {
                let wake = poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
                let _binding = schedule.bind(&self.cpu.borrow(), &wake);
                let hardware = Hardware::new(wake);
                self.hardware = Rc::downgrade(&hardware);
                let execution = self.cpu.borrow().execution.clone();
                let running = Rc::new(RefCell::new(None::<CpuRun>));
                self.running = Rc::downgrade(&running);
                let mut devices = pin!(inbox.for_each_concurrent(None, |activity| activity));
                let mut application = pin!(main(self));

                poll_fn(|cx| {
                    hardware.deliver_input();
                    hardware.fire_alarms();
                    let _ = devices.as_mut().poll(cx);
                    if let Poll::Ready(result) = application.as_mut().poll(cx) {
                        return Poll::Ready(result);
                    }
                    let _permit = match &execution {
                        Some(member) => Some(ready!(member.poll_enter(cx))),
                        None => None,
                    };
                    let mut active = running.borrow_mut();
                    let turn = {
                        let mut cpu = active.as_ref().map(|run| run.cpu.borrow_mut());
                        // A BIOS hook may finish while host pacing prevents another guest block.
                        if let Some(exit) = cpu.as_mut().and_then(|cpu| cpu.poll_host_work(cx)) {
                            CpuStop::Exit(exit)
                        } else {
                            ready!(schedule.poll_pacing(cx));
                            cpu.map_or(CpuStop::Blocked, |mut cpu| cpu.run_to_boundary(cx))
                        }
                    };
                    match turn {
                        CpuStop::Checkpoint => schedule.notify_due(),
                        CpuStop::Exit(exit) => {
                            schedule.notify_due();
                            let run = active.take().expect("only a running CPU can exit");
                            let _ = run.completed.send(exit);
                        }
                        CpuStop::Blocked => {
                            if executor.has_pending_poll() || !schedule.advance_idle() {
                                return Poll::Pending;
                            }
                        }
                    }
                    executor.request_poll();
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
        let Some(running) = self.running.upgrade() else {
            return ExecutionExit::Interrupted;
        };
        let (completed, exit) = oneshot::channel();
        running.replace(Some(CpuRun {
            cpu: self.cpu.clone(),
            completed,
        }));
        self.executor.request_poll();
        drop(running);
        let _cancel = CancelRun(self);
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
        Self::with_schedule(ExecutionSchedule::with_guest_time(config), cpu, endpoint).await
    }
}
