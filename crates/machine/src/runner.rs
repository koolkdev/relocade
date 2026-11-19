//! CPU execution and registered device tasks on one owner.

mod worker;
pub use worker::ExecutionWorker;

use crate::scheduler::{GuestTimeConfig, Scheduler, SessionTask, Timing};
use crate::worker::{HostTurns, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker, WorkerWake};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
/// Gives other ready Rust tasks a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::future::{join_all, LocalBoxFuture};
use relocade_x86::ExecutionExit;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::{Rc, Weak},
};

/// Owns a worker's execution session, including its CPU backend and device tasks.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust tasks can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    clock: Clock,
    new_tasks: mpsc::UnboundedSender<SessionTask>,
    session: Option<Scheduler>,
    pub(crate) running: Weak<RefCell<Option<CpuRun>>>,
    wake: Rc<WorkerWake<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Rc<RefCell<Cpu>>,
}

pub(crate) struct CpuRun {
    pub(crate) cpu: Rc<RefCell<Cpu>>,
    pub(crate) completed: oneshot::Sender<ExecutionExit>,
}

// Cancelling the foreground wait returns exclusive access to Runner::cpu immediately.
struct CancelRun<'a>(&'a Runner);

impl Drop for CancelRun<'_> {
    fn drop(&mut self) {
        if let Some(running) = self.0.running.upgrade() {
            if running.take().is_some() {
                self.0.wake.request_poll();
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
        Self::with_schedule(Timing::from_clock(clock), cpu, endpoint).await
    }

    async fn with_schedule(
        time: Timing,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        let wake = Rc::new(WorkerWake::new(endpoint.bind(cpu.interrupt_handle())));
        let (cpu, compiler) = cpu.into_session(IoBus::default()).await?;
        let (new_tasks, incoming_tasks) = mpsc::unbounded();
        let compiler = compiler
            .map(|endpoint| {
                let (client, replies) = endpoint.connect(&wake)?;
                new_tasks
                    .unbounded_send(replies)
                    .expect("new session task queue");
                Ok::<_, crate::Error>(client)
            })
            .transpose()?;
        let cpu = cpu
            .with_compiler(compiler)
            .with_host_turns(wake.host_turns());
        Ok(Self {
            clock: time.clock().clone(),
            new_tasks,
            session: Some(Scheduler::new(time, incoming_tasks, wake.clone())),
            running: Weak::new(),
            wake,
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
        self.wake.peer()
    }

    /// Declares an operation whose completion needs this worker's host event loop.
    /// Clone before awaiting a browser promise alongside runnable guest work.
    pub fn host_turns(&self) -> HostTurns {
        self.wake.host_turns()
    }

    /// Starts a reusable calculation thread owned by this execution session.
    /// Returns after the thread binds the session's wake peer. Await during setup
    /// so browser startup can finish before synchronous guest execution begins.
    /// The session retains the worker if this startup wait is cancelled.
    /// Each submitted closure owns its inputs and returns its result asynchronously.
    /// The submitting task can await a result while guest execution continues.
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

    /// Registers a device's mappings, then starts its task on this worker.
    /// On return, the task has completed its first poll and had a chance to register its waits.
    /// Once queued, it belongs to the session even if this wait is cancelled.
    /// A device that only handles bus accesses can register directly through [`IoBus::connect`].
    pub async fn connect<D: Device + 'static>(
        &self,
        device: Rc<RefCell<D>>,
        configure: impl FnOnce(&mut DeviceConnection<'_>),
    ) {
        self.io().connect(device.clone(), configure);
        self.start_task(D::run(device)).await;
    }

    /// Starts a Rust task on this execution worker and returns after its first poll.
    /// The session owns the queued task even if the caller drops this startup wait.
    /// The scheduler polls the task when woken and drops it when the session ends.
    pub async fn start_task(&self, task: impl Future<Output = ()> + 'static) {
        let (acknowledge, started) = oneshot::channel();
        let session_task = async move {
            let mut task = pin!(task);
            let mut acknowledge = Some(acknowledge);
            poll_fn(|cx| {
                let result = task.as_mut().poll(cx);
                if let Some(acknowledge) = acknowledge.take() {
                    let _ = acknowledge.send(());
                }
                result
            })
            .await;
        };
        self.new_tasks
            .unbounded_send(Box::pin(session_task))
            .expect("running session");
        started.await.expect("session must poll queued tasks");
    }

    /// Runs the application and its background tasks for one execution session.
    /// Devices remain active while the application awaits input or another
    /// process. Completion or cancellation drops the session’s tasks on this worker,
    /// then drains calculations and joins their threads.
    /// The stop future is registered before application code runs. It returns
    /// `None` on cancellation; ordinary main completion returns `Some(result)`.
    pub async fn run<R>(
        mut self,
        stop: impl Future,
        main: impl AsyncFnOnce(Self) -> R,
    ) -> Option<R> {
        let scheduler = self.session.take().expect("new execution session");
        let workers = self.workers.clone();
        let result = scheduler.run(self, stop, main).await;
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
        self.wake.request_poll();
        drop(running);
        let _cancel = CancelRun(self);
        exit.await.unwrap_or(ExecutionExit::Interrupted)
    }

    /// Runs devices on guest time. An optional CPU throughput limit applies to
    /// retired instructions. Device deadlines remain active between foreground runs.
    pub async fn with_guest_time(
        config: GuestTimeConfig,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        Self::with_schedule(Timing::with_guest_time(config), cpu, endpoint).await
    }
}
