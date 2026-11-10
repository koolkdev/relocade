//! CPU execution and registered device activities on one owner.

mod guest_time;
mod schedule;
pub use guest_time::GuestTimeConfig;
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
    storage: Option<CpuStorage>,
    cpu: Option<Cpu>,
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
        Self::with_schedule(ExecutionSchedule::from_clock(clock), storage, endpoint)
    }

    fn with_schedule(
        schedule: ExecutionSchedule,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Self {
        let (activities, pending) = mpsc::unbounded();
        let executor = Rc::new(Executor::new(endpoint.bind(storage.interrupt_handle())));
        Self {
            schedule,
            activities,
            pending: Some(pending),
            executor,
            storage: Some(storage),
            cpu: None,
        }
    }

    /// Constructs and retains the session CPU from its prepared storage once.
    /// Subsequent programs reuse that CPU. Use [`Cpu::new`] for independent CPU-level work.
    ///
    /// # Panics
    /// Panics if the storage was consumed by an earlier
    /// construction attempt, including one that failed or was cancelled.
    pub async fn create_cpu(
        &mut self,
        memory: Memory,
        config: CpuConfig,
        io: IoBus,
    ) -> Result<&mut Cpu> {
        let storage = self
            .storage
            .take()
            .expect("session CPU storage already consumed");
        self.cpu = Some(storage.into_cpu(memory, config, io).await?);
        Ok(self.cpu())
    }

    /// Borrows this runner's stopped CPU for setup and low-level execution.
    /// The borrow must end before calling [`Self::run_cpu`].
    pub fn cpu(&mut self) -> &mut Cpu {
        self.cpu.as_mut().expect("construct the session CPU first")
    }

    pub fn clock(&self) -> &Clock {
        self.schedule.clock()
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
    /// Poll within [`Self::run`], which drives the session's scheduling and devices.
    ///
    /// # Panics
    /// Panics if the session CPU has not been constructed.
    pub async fn run_cpu(&mut self) -> ExecutionExit {
        let cpu = self.cpu.as_mut().expect("construct the session CPU first");
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
        Self::with_schedule(
            ExecutionSchedule::with_guest_time(config),
            storage,
            endpoint,
        )
    }
}
