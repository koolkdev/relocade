//! CPU execution and registered device activities on one owner.

mod guest_time;
mod schedule;
pub use guest_time::GuestTimeConfig;
mod worker;

use schedule::ExecutionSchedule;
pub use worker::ExecutionWorker;

use crate::worker::{Executor, WakeEndpoint, WakePeer};
use crate::{
    Clock, Cpu, CpuHandle, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
use futures_lite::future::poll_once;
/// Gives other ready Rust activities a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::{
    future::{select, Either, LocalBoxFuture},
    StreamExt,
};
use relocade_x86::ExecutionExit;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::Poll,
    time::Duration,
};

/// Owns a worker's execution session, including its CPU backend and device activities.
/// [`crate::Machine`] prepares CPU state before [`ExecutionWorker`] transfers it here.
/// Notifications interrupt guest execution so ready Rust activities can run.
pub struct Runner {
    schedule: ExecutionSchedule,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    cpu: Cpu,
    compiler: Option<crate::CompilerClient>,
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
        Ok(Self {
            schedule,
            activities,
            pending: Some(pending),
            executor,
            cpu,
            compiler,
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
    /// Returns `None` if no service was supplied at CPU creation or it was already taken.
    /// The session receives ordered results through this single owner.
    pub fn take_compiler(&mut self) -> Option<crate::CompilerClient> {
        self.compiler.take()
    }

    pub fn clock(&self) -> &Clock {
        self.schedule.clock()
    }

    /// Post this resource to another worker and retain its `bind()` guard there
    /// before sending ordinary Rust notifications to this execution session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.executor.peer()
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
        let (started, ready) = oneshot::channel();
        let activity = async move {
            let mut run = pin!(D::run(device));
            let completed = poll_once(run.as_mut()).await.is_some();
            let _ = started.send(());
            if !completed {
                run.await;
            }
        };
        self.activities
            .unbounded_send(Box::pin(activity))
            .unwrap_or_else(|_| panic!("runner must own its device activities"));
        ready.await.expect("runner must poll registered devices");
    }

    /// Runs the main activity and devices for one execution lifetime.
    /// Devices remain active while the main activity awaits input or another
    /// process. Completion or cancellation drops every activity on this owner.
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
        executor
            .run_until(stop, async {
                // Poll ready devices before returning to guest execution.
                match select(pin!(devices), pin!(main(self))).await {
                    Either::Right((result, _)) => result,
                    Either::Left(_) => unreachable!("session retains the activity sender"),
                }
            })
            .await
    }

    /// Runs until the guest exits, polling ready Rust work on execution interrupts.
    /// Pending services retain their futures while awaiting input or guest resumption.
    /// Poll within [`Self::run`], which drives scheduling and background activities.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let cpu = &mut self.cpu;
        let changed = poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
        let _binding = self.schedule.bind(cpu, &changed);
        let interrupt = cpu.interrupt_handle();
        self.executor.yield_now().await;
        loop {
            let exit = poll_fn(|cx| self.schedule.poll_cpu(cpu, cx)).await;
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
    /// Time pauses between calls to [`Self::run_cpu`]; existing pacing credit and
    /// the host/guest offset survive those pauses.
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
