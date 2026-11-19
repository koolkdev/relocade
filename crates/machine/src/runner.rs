//! CPU execution and registered device activities on one owner.

mod guest_time;
mod schedule;
mod session;
pub use guest_time::GuestTimeConfig;
mod worker;

use schedule::ExecutionSchedule;
use session::Session;
pub use worker::ExecutionWorker;

use crate::worker::{Executor, HostTurns, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, HostTimerDriver, IoBus,
    Memory, Result,
};
use futures_channel::oneshot;
/// Gives other ready Rust activities a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::future::{join_all, select, Either, LocalBoxFuture};
use relocade_x86::ExecutionExit;
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::{Rc, Weak},
    time::Duration,
};

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    clock: Clock,
    session: Weak<Session>,
    schedule: Option<ExecutionSchedule>,
    delivery_timers: Option<HostTimerDriver>,
    compiler_replies: Option<LocalBoxFuture<'static, ()>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Rc<RefCell<Cpu>>,
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
        Self::with_schedule(ExecutionSchedule::from_clock(clock), cpu, endpoint, None).await
    }

    /// The stock host-timed session shares its real timer worker with browser delivery.
    pub(crate) async fn with_host_timers(
        timers: crate::TimerHandle,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        let clock = timers.clock();
        Self::with_schedule(
            ExecutionSchedule::from_clock(clock),
            cpu,
            endpoint,
            Some(timers),
        )
        .await
    }

    async fn with_schedule(
        schedule: ExecutionSchedule,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
        host_timers: Option<crate::TimerHandle>,
    ) -> Result<Self> {
        let mut executor = Executor::new(endpoint.bind(cpu.interrupt_handle()));
        // Browser delivery must progress even when an embedding freezes its
        // device or pacing clock. Only an actual timer service can supply it.
        let (host_timers, delivery_timers) = match host_timers {
            None if cfg!(target_arch = "wasm32") => {
                let (handle, timers) = crate::timer_service();
                (Some(handle), Some(timers))
            }
            handle => (handle, None),
        };
        if let Some(timers) = host_timers {
            let clock = timers.clock();
            executor =
                executor.with_host_timer(Duration::from_millis(1), move |delay| clock.sleep(delay));
        }
        let executor = Rc::new(executor);
        let (cpu, compiler) = cpu.into_session(IoBus::default()).await?;
        let (compiler, compiler_replies) = compiler
            .map(|compiler| compiler.connect(&executor))
            .transpose()?
            .unzip();
        let cpu = cpu
            .with_compiler(compiler)
            .with_host_turns(executor.host_turns());
        Ok(Self {
            clock: schedule.clock().clone(),
            session: Weak::new(),
            schedule: Some(schedule),
            delivery_timers,
            compiler_replies,
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
        let session = self
            .session
            .upgrade()
            .expect("activity needs a running session");
        let (started, ready) = oneshot::channel();
        session.spawn(&self.executor, Box::pin(activity), Some(started));
        drop(session);
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
        let workers = self.workers.clone();
        let result = async {
            let mut stop = pin!(stop);
            if let Some(timers) = self.delivery_timers.take() {
                // The timer worker's startup needs JavaScript on this owner.
                // Complete it before any guest or device can stay runnable.
                let (started, ready) = oneshot::channel();
                let worker = timers.spawn_bound(self.executor.peer(), move || {
                    let _ = started.send(());
                });
                workers.borrow_mut().push(Box::pin(async move {
                    worker.stop().await.expect("session timer cleanup");
                }));
                match select(stop.as_mut(), ready).await {
                    Either::Left(_) => return None,
                    Either::Right((started, _)) => started.expect("session timer startup"),
                }
            }
            let schedule = self.schedule.take().expect("new execution session");
            let session = Session::new(&self.executor);
            if let Some(replies) = self.compiler_replies.take() {
                session.spawn(&self.executor, replies, None);
            }
            self.session = Rc::downgrade(&session);
            let execution = self.cpu.borrow().execution.clone();
            let _binding = schedule.bind(&self.cpu.borrow(), &session.machine);
            let executor = self.executor.clone();
            session
                .drive(&executor, stop, main(self), schedule, execution)
                .await
        }
        .await;
        let cleanup = std::mem::take(&mut *workers.borrow_mut());
        join_all(cleanup).await;
        result
    }

    /// Requests guest execution and awaits its exit inside [`Self::run`].
    /// Dropping the wait stops execution and preserves pending CPU services.
    /// Ending the session interrupts an outstanding wait.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let Some(session) = self.session.upgrade() else {
            return ExecutionExit::Interrupted;
        };
        let (registration, completed) = session.run_cpu(self.cpu.clone());
        drop(session);
        let exit = completed.await.unwrap_or(ExecutionExit::Interrupted);
        drop(registration);
        exit
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
        Self::with_schedule(
            ExecutionSchedule::with_guest_time(config),
            cpu,
            endpoint,
            None,
        )
        .await
    }
}
