//! Runs one CPU session and its device tasks on the execution worker.

mod config;
mod execution;
mod guest_time;
mod timing;
mod worker;

pub use config::GuestTimeConfig;
pub use guest_time::GuestTimeDriver;
use timing::Timing;
pub use worker::{ExecutionWorker, SessionConfig};

use crate::worker::{HostTurns, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker, WorkerWake};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
/// Gives other ready Rust tasks a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::{
    future::{join_all, LocalBoxFuture},
    StreamExt,
};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::Poll,
};

type SessionTask = LocalBoxFuture<'static, ()>;

/// Owns a worker's execution session, including its CPU backend and device tasks.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Use the execution and idle waits inside the callback passed to [`Self::run`].
/// Notifications interrupt guest execution so ready Rust tasks can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    time: Timing,
    new_tasks: mpsc::UnboundedSender<SessionTask>,
    tasks: Option<mpsc::UnboundedReceiver<SessionTask>>,
    wake: Rc<WorkerWake<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Cpu,
}

impl Runner {
    /// Runs the application and its background tasks for one execution session.
    /// Ready device and compiler tasks run throughout the session. The foreground
    /// advances guest time with [`Self::wait_for`] or [`Self::run_cpu`].
    /// Completion or cancellation drops the tasks, drains calculations and joins their threads.
    /// The stop future is registered before application code runs. It returns
    /// `None` on cancellation; ordinary main completion returns `Some(result)`.
    pub async fn run<R>(
        mut self,
        stop: impl Future,
        main: impl AsyncFnOnce(Self) -> R,
    ) -> Option<R> {
        let tasks = self.tasks.take().expect("new execution session");
        let workers = self.workers.clone();
        let wake = self.wake.clone();
        let _clock = self.time.bind(&self.cpu, wake.waker());
        let result = {
            let mut stop = pin!(stop);
            let mut tasks = pin!(tasks.for_each_concurrent(None, |task| task));
            let mut main = pin!(main(self));
            wake.drive(poll_fn(|cx| {
                if stop.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                let _ = tasks.as_mut().poll(cx);
                main.as_mut().poll(cx).map(Some)
            }))
            .await
        };
        let cleanup = std::mem::take(&mut *workers.borrow_mut());
        join_all(cleanup).await;
        result
    }

    /// Constructs the local CPU backend and binds its wake endpoint.
    /// The supplied clock must progress independently of this session. Use
    /// [`Self::with_guest_time`] for a clock advanced by CPU execution.
    /// Supply the endpoint from this CPU's wake connection and retain the host
    /// until the session ends.
    pub async fn new(
        clock: Clock,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        Self::initialize(cpu, endpoint, clock, None).await
    }

    /// Runs devices on guest time. The host clock times idle waits, presentation,
    /// and any CPU throughput limit configured for this session.
    pub async fn with_guest_time(
        config: GuestTimeConfig,
        host: Clock,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        Self::initialize(cpu, endpoint, host, Some(config)).await
    }

    async fn initialize(
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
        host: Clock,
        guest: Option<GuestTimeConfig>,
    ) -> Result<Self> {
        let time = match guest {
            Some(config) => {
                GuestTimeDriver::validate_target(cpu.configuration().execution_target())?;
                Timing::with_guest_time(config, host)
            }
            None => Timing::from_clock(host),
        };
        let wake = Rc::new(WorkerWake::new(endpoint.bind(cpu.interrupt_handle())));
        let (cpu, compiler) = cpu.into_cpu_and_compiler(IoBus::default()).await?;
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
            time,
            new_tasks,
            tasks: Some(incoming_tasks),
            wake,
            workers: Rc::new(RefCell::new(Vec::new())),
            cpu,
        })
    }

    /// The shared memory established by the machine before execution started.
    pub fn memory(&self) -> Memory {
        self.cpu.memory()
    }

    /// The session's device bus for setup and synchronous host I/O.
    pub fn io(&self) -> IoBus {
        self.cpu.io()
    }

    /// Borrows this runner's stopped CPU for setup and low-level execution.
    /// The borrow must end before calling [`Self::run_cpu`].
    pub fn cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
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
        self.time.clock()
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
        self.host_turns()
            .wait(ready)
            .await
            .expect("session job thread startup");
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
}
