//! CPU execution and registered device activities on one owner.

mod deadlines;
pub use deadlines::DeviceDeadline;
use deadlines::DeviceDeadlines;

mod guest_time;
mod schedule;
mod session;
pub use guest_time::GuestTimeConfig;
mod worker;

use schedule::ExecutionSchedule;
use session::{Session, SessionScope};
pub use worker::ExecutionWorker;

use crate::worker::{Executor, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, GuestClock, IoBus, Memory,
    Result,
};
use futures_channel::{mpsc, oneshot};
use futures_lite::future::poll_once;
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
    num::NonZeroU64,
    pin::pin,
    rc::Rc,
    task::Poll,
    time::Duration,
};

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    schedule: Rc<ExecutionSchedule>,
    session: Rc<RefCell<Option<Session>>>,
    deadlines: std::rc::Weak<RefCell<DeviceDeadlines>>,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Option<Cpu>,
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
            schedule: Rc::new(schedule),
            session: Rc::default(),
            deadlines: std::rc::Weak::new(),
            activities,
            pending: Some(pending),
            executor,
            workers: Rc::new(RefCell::new(Vec::new())),
            cpu: Some(cpu),
        })
    }

    /// The shared memory established by the machine before execution started.
    pub fn memory(&self) -> Memory {
        self.cpu
            .as_ref()
            .expect("CPU available outside a run")
            .memory()
    }

    /// The session's device bus for setup and synchronous host I/O.
    pub fn io(&self) -> IoBus {
        self.cpu.as_ref().expect("CPU available outside a run").io()
    }

    /// Borrows this runner's stopped CPU for setup and low-level execution.
    /// The borrow must end before calling [`Self::run_cpu`].
    pub fn cpu(&mut self) -> &mut Cpu {
        self.cpu.as_mut().expect("CPU available outside a run")
    }

    /// Takes this CPU's compiler connection once.
    /// Returns `None` if no service was supplied, the connection closed, or it was taken.
    /// The session receives ordered results through this single owner.
    /// Take it during setup to manage requests and installation explicitly.
    /// Panics if automatic compilation requests are still outstanding.
    pub fn take_compiler(&mut self) -> Option<crate::CompilerClient> {
        self.cpu().take_compiler()
    }

    /// Sets up hardware on one guest timeline. After the first successful setup,
    /// a live session drives that clock between foreground runs. A supplied
    /// external guest clock keeps its own controller throughout setup and execution.
    /// Call inside [`Self::run`] when installing device activities.
    pub async fn initialize_guest_devices<T, E>(
        mut self,
        instructions_per_second: NonZeroU64,
        initialize: impl AsyncFnOnce(Self, GuestClock) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let clock = match self.clock().as_guest() {
            Some(clock) => clock,
            None => {
                let config = GuestTimeConfig::new(instructions_per_second)
                    .with_host_clock(self.clock().clone());
                self.schedule = Rc::new(ExecutionSchedule::with_guest_time(config));
                self.clock()
                    .as_guest()
                    .expect("guest schedule has a guest clock")
            }
        };
        let slot = self.session.clone();
        let driver = Session::new(
            self.cpu.as_ref().unwrap(),
            self.schedule.clone(),
            self.executor.clone(),
        );
        let hardware = initialize(self, clock).await?;
        slot.borrow_mut().get_or_insert(driver);
        Ok(hardware)
    }

    pub fn clock(&self) -> &Clock {
        self.schedule.clock()
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
        D::register_deadlines(&device, self);
        self.spawn_activity(D::run(device)).await;
    }

    /// Schedules hardware on this session's clock. Each due transition receives
    /// the current time and returns its next deadline. Use the returned handle
    /// to change that deadline when a guest register write reprograms the device.
    pub fn schedule_device(
        &self,
        clock: &GuestClock,
        initial: Option<crate::Timestamp>,
        advance: impl FnMut(crate::Timestamp) -> Option<crate::Timestamp> + 'static,
    ) -> DeviceDeadline {
        assert!(
            self.clock().shares_source(clock),
            "scheduled devices must use the runner's guest clock"
        );
        self.deadlines
            .upgrade()
            .expect("schedule devices inside Runner::run")
            .borrow_mut()
            .register(&self.schedule, initial, advance)
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
        let pending = self.pending.take().expect("new runner activity queue");
        let devices = pending.for_each_concurrent(None, |activity| activity);
        let executor = self.executor.clone();
        let _activities = self.activities.clone();
        let workers = self.workers.clone();
        let session = SessionScope(self.session.clone());
        let deadlines = Rc::new(RefCell::new(DeviceDeadlines::default()));
        self.deadlines = Rc::downgrade(&deadlines);
        let result = executor
            .run_until(stop, async {
                let mut devices = pin!(devices);
                let mut application = pin!(main(self));
                poll_fn(|cx| {
                    let _ = devices.as_mut().poll(cx);
                    if let Poll::Ready(result) = application.as_mut().poll(cx) {
                        return Poll::Ready(result);
                    }
                    deadlines.borrow_mut().poll(cx);
                    if let Some(session) = session.0.borrow_mut().as_mut() {
                        session.poll(cx);
                    }
                    Poll::Pending
                })
                .await
            })
            .await;
        drop(deadlines);
        drop(session);
        let cleanup = std::mem::take(&mut *workers.borrow_mut());
        join_all(cleanup).await;
        result
    }

    /// Runs until the guest exits, polling ready Rust work on execution interrupts.
    /// Pending services retain their futures while awaiting input or guest resumption.
    /// Poll within [`Self::run`], which drives scheduling and background activities.
    /// If a foreground waiter outlives its live session, it returns Interrupted.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let foreground = self
            .session
            .borrow()
            .as_ref()
            .map(|session| session.foreground.clone());
        if let Some(foreground) = foreground {
            return foreground.run(&mut self.cpu).await;
        }
        let cpu = self.cpu.as_mut().unwrap();
        let execution = cpu.execution.clone();
        let changed = poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
        let _binding = self
            .schedule
            .bind(cpu.cpu_state(), cpu.instruction_target(), &changed);
        let interrupt = cpu.interrupt_handle();
        self.executor.yield_now().await;
        loop {
            let exit =
                poll_fn(|cx| self.schedule.poll_turn(Some(cpu), execution.as_ref(), cx)).await;
            if exit.is_none() || interrupt.is_requested() {
                self.executor.yield_now().await;
            }
            if let Some(exit) = exit {
                return exit;
            }
        }
    }

    /// Runs on emulated guest time with optional host pacing for active and idle
    /// execution. Device deadlines subdivide each execution slice. Instruction targets
    /// remain soft checkpoint limits, so a slice can overshoot.
    /// CPU-only runs pause guest time between submissions. A live session created
    /// by [`Self::initialize_guest_devices`] continues advancing to device deadlines.
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
