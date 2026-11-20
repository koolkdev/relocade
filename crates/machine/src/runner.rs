//! CPU execution and registered device activities on one owner.

mod guest_time;
mod schedule;
pub use guest_time::GuestTimeConfig;
mod worker;

use schedule::ExecutionSchedule;
pub use worker::{ExecutionWorker, SessionConfig};

use crate::worker::{HostTurns, JobExecutor, JobQueue, WakeEndpoint, WakePeer, Worker, WorkerWake};
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
    StreamExt,
};
use relocade_x86::ExecutionExit;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::Poll,
};

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
/// An optional compiler connection automatically compiles hot Real16 functions.
/// Protected-mode execution does not collect call profiles or request compilation.
pub struct Runner {
    schedule: ExecutionSchedule,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    wake: Rc<WorkerWake<ExecutionInterrupt>>,
    workers: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    cpu: Cpu,
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
        let (activities, pending) = mpsc::unbounded();
        let wake = Rc::new(WorkerWake::new(endpoint.bind(cpu.interrupt_handle())));
        let (cpu, compiler) = cpu.into_session(IoBus::default()).await?;
        let compiler = compiler
            .map(|endpoint| {
                let (client, replies) = endpoint.connect(&wake)?;
                activities
                    .unbounded_send(replies)
                    .expect("new session task queue");
                Ok::<_, crate::Error>(client)
            })
            .transpose()?;
        let cpu = cpu
            .with_compiler(compiler)
            .with_host_turns(wake.host_turns());
        Ok(Self {
            schedule,
            activities,
            pending: Some(pending),
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
        self.cpu.take_compiler()
    }

    pub fn clock(&self) -> &Clock {
        self.schedule.clock()
    }

    /// Post this resource to another worker and retain its `bind()` guard there
    /// before sending ordinary Rust notifications to this execution session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.wake.peer()
    }

    /// Declares an operation whose completion needs this worker's host event loop.
    pub fn host_turns(&self) -> HostTurns {
        self.wake.host_turns()
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
        self.host_turns()
            .wait(ready)
            .await
            .expect("session job thread startup");
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
        let wake = self.wake.clone();
        let workers = self.workers.clone();
        let result = {
            let mut stop = pin!(stop);
            let mut devices = pin!(pending.for_each_concurrent(None, |activity| activity));
            let mut main = pin!(main(self));
            wake.drive(poll_fn(|cx| {
                if stop.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                let _ = devices.as_mut().poll(cx);
                main.as_mut().poll(cx).map(Some)
            }))
            .await
        };
        let cleanup = std::mem::take(&mut *workers.borrow_mut());
        join_all(cleanup).await;
        result
    }

    /// Runs until the guest exits, polling ready Rust work on execution interrupts.
    /// Pending services retain their futures while awaiting input or guest resumption.
    /// Poll within [`Self::run`], which drives scheduling and background activities.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let cpu = &mut self.cpu;
        let interrupt = cpu.interrupt_handle();
        let mut binding = None;
        let exit = poll_fn(|cx| {
            if binding.is_none() {
                binding = Some(self.schedule.bind(cpu, cx.waker()));
            } else if let Some(exit) =
                std::task::ready!(self.schedule.poll_cpu(cpu, &self.wake, cx))
            {
                return Poll::Ready(exit);
            }
            self.wake.repoll(cx);
            Poll::Pending
        })
        .await;
        if interrupt.is_requested() {
            yield_now().await;
        }
        exit
    }

    /// Runs on emulated guest time with optional host pacing for active and idle
    /// execution. Device deadlines subdivide each execution slice. Instruction targets
    /// remain soft checkpoint limits, so a slice can overshoot.
    /// Time pauses between calls to [`Self::run_cpu`]; existing pacing credit and
    /// the host/guest offset survive those pauses.
    pub async fn with_guest_time(
        config: GuestTimeConfig,
        cpu: CpuHandle,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Result<Self> {
        crate::GuestTimeDriver::validate_target(cpu.configuration().execution_target())?;
        Self::with_schedule(ExecutionSchedule::with_guest_time(config), cpu, endpoint).await
    }
}
