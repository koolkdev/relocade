//! CPU execution and registered device activities on one owner.

mod compilation;
mod guest_time;
mod host;
mod schedule;
mod session;
pub use guest_time::GuestTimeConfig;
pub use session::run_session;
mod worker;

use schedule::ExecutionSchedule;
pub use worker::ExecutionWorker;

use crate::worker::{Executor, WakeEndpoint, WakePeer};
use crate::{
    Clock, Cpu, CpuConfig, CpuStorage, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory,
    Result,
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
    ops::RangeInclusive,
    pin::pin,
    rc::Rc,
    task::Poll,
};

/// Owns a worker's execution session, including its CPU storage and device activities.
/// [`ExecutionWorker`] prepares the resources before starting the owner thread.
/// Notifications interrupt guest execution so ready Rust activities can run.
pub struct Runner {
    schedule: ExecutionSchedule,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    storage: RefCell<Option<CpuStorage>>,
    compilation: Option<compilation::Compilation>,
}

impl Runner {
    /// Binds prepared CPU storage and its wake endpoint on the execution owner.
    /// Normally called by [`ExecutionWorker`]. The endpoint must come from a
    /// connection created for this storage; retain its host until the session ends.
    pub fn new(
        clock: Clock,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Self {
        Self::with_schedule(
            ExecutionSchedule::from_clock(clock),
            storage,
            endpoint,
            None,
        )
    }

    pub(super) fn with_host_timers(
        timers: crate::TimerHandle,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Self {
        let clock = timers.clock();
        Self::with_schedule(
            ExecutionSchedule::from_clock(clock.clone()),
            storage,
            endpoint,
            Some(clock),
        )
    }

    fn with_schedule(
        schedule: ExecutionSchedule,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
        host_clock: Option<Clock>,
    ) -> Self {
        let (activities, pending) = mpsc::unbounded();
        let mut executor = Executor::new(endpoint.bind(storage.interrupt_handle()));
        if let Some(clock) = host_clock {
            executor = executor.with_host_timer(move |delay| clock.sleep(delay));
        }
        let executor = Rc::new(executor);
        Self {
            schedule,
            activities,
            pending: Some(pending),
            executor,
            storage: RefCell::new(Some(storage)),
            compilation: None,
        }
    }

    /// Constructs the session CPU from its prepared storage once. Subsequent
    /// programs reuse that CPU. Use [`Cpu::new`] for independent CPU-level work.
    ///
    /// # Panics
    /// Panics if the storage was consumed by an earlier
    /// construction attempt, including one that failed or was cancelled.
    pub async fn create_cpu(&self, memory: Memory, config: CpuConfig, io: IoBus) -> Result<Cpu> {
        let storage = self
            .storage
            .borrow_mut()
            .take()
            .expect("session CPU storage already consumed");
        let mut cpu = storage.into_cpu(memory, config, io).await?;
        cpu.executor = Rc::downgrade(&self.executor);
        if let Some(compilation) = &self.compilation {
            compilation.register(&cpu);
        }
        Ok(cpu)
    }

    pub fn clock(&self) -> &Clock {
        self.schedule.clock()
    }

    /// Creates a compiler connection with receipt bound to this execution owner.
    /// Transfer the connection to its host and attach it to a `CompilerWorker`;
    /// the returned client can then await compilation directly.
    pub fn compiler_connection(
        &self,
    ) -> Result<(crate::CompilerClient, crate::CompilerConnection)> {
        crate::compiler::channel(|endpoint| self.bind_resource(endpoint))
    }

    /// Post this resource to another worker and retain its `bind()` guard there
    /// before sending ordinary Rust notifications to this execution session.
    pub fn wake_peer(&self) -> WakePeer<ExecutionInterrupt> {
        self.executor.peer()
    }

    /// Starts a device's activity before returning its bus connection.
    /// Await this during setup inside [`Self::run`]. The activity has completed
    /// its first poll with its device waker before this returns; pending waits
    /// can therefore notify execution before the first guest instruction.
    /// Initialize bus state before connecting, including any asynchronous setup.
    /// Once queued, the activity belongs to the session even if this wait is
    /// cancelled. The returned connection belongs in the CPU's [`crate::IoBus`].
    pub async fn connect<D: Device + 'static>(
        &self,
        device: Rc<RefCell<D>>,
        mmio: impl IntoIterator<Item = RangeInclusive<u32>>,
        ports: impl IntoIterator<Item = RangeInclusive<u16>>,
    ) -> DeviceConnection {
        let (started, ready) = oneshot::channel();
        let activity_device = device.clone();
        let activity = async move {
            let mut run = pin!(D::run(activity_device));
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
        DeviceConnection {
            device,
            mmio: mmio.into_iter().collect(),
            ports: ports.into_iter().collect(),
        }
    }

    /// Runs the main activity and devices for one execution lifetime.
    /// Devices remain active while the main activity awaits input or another
    /// process. Completion or cancellation drops every activity on this owner.
    /// The stop future is registered before application code runs. It returns
    /// `None` on cancellation; ordinary main completion returns `Some(result)`.
    pub async fn run<R>(
        mut self,
        stop: impl Future,
        main: impl AsyncFnOnce(&Self) -> R,
    ) -> Option<R> {
        let pending = self.pending.take().expect("new runner activity queue");
        let devices = pending.for_each_concurrent(None, |activity| activity);
        self.executor
            .run_until(stop, async {
                // Poll ready devices before returning to guest execution.
                match select(pin!(devices), pin!(main(&self))).await {
                    Either::Right((result, _)) => result,
                    Either::Left(_) => unreachable!("runner retains the activity sender"),
                }
            })
            .await
    }

    /// Runs until the guest exits, polling ready Rust work on execution interrupts.
    /// Pending services retain their futures while awaiting input or guest resumption.
    /// Poll within [`Self::run`], which drives the session's scheduling and devices.
    ///
    /// # Panics
    /// Panics if the CPU was not created by this runner.
    pub async fn run_cpu(&self, cpu: &mut Cpu) -> ExecutionExit {
        assert!(
            std::ptr::eq(cpu.executor.as_ptr(), Rc::as_ptr(&self.executor)),
            "construct the session CPU with runner.create_cpu"
        );
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
    pub fn with_guest_time(
        config: GuestTimeConfig,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Self {
        let host_clock = config.host_clock.clone();
        Self::with_schedule(
            ExecutionSchedule::with_guest_time(config),
            storage,
            endpoint,
            host_clock,
        )
    }
}
