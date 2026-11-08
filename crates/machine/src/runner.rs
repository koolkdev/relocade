//! CPU execution and registered device activities on one owner.

mod instruction;
mod worker;
use instruction::{InstructionSchedule, RunProgress};
pub use worker::ExecutionWorker;

use crate::worker::{Executor, WakeEndpoint, WakePeer};
use crate::{
    Clock, Cpu, CpuConfig, CpuStorage, Device, DeviceConnection, ExecutionInterrupt,
    InstructionTimeline, IoBus, Memory, Result,
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
    num::NonZeroU64,
    ops::RangeInclusive,
    pin::pin,
    rc::Rc,
    time::Duration,
};

/// Owns a worker's execution session, including its CPU storage and device activities.
/// [`ExecutionWorker`] prepares the resources before starting the owner thread.
/// Notifications interrupt guest execution so ready Rust activities can run.
pub struct Runner {
    clock: Clock,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    storage: RefCell<Option<CpuStorage>>,
    instruction_schedule: Option<InstructionSchedule>,
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
        let (activities, pending) = mpsc::unbounded();
        let executor = Rc::new(Executor::new(endpoint.bind(storage.interrupt_handle())));
        Self {
            clock,
            activities,
            pending: Some(pending),
            executor,
            storage: RefCell::new(Some(storage)),
            instruction_schedule: None,
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
        if let Some(schedule) = &self.instruction_schedule {
            schedule.timeline.bind_cpu(cpu.cpu_state());
        }
        Ok(cpu)
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
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
        let _schedule = self
            .instruction_schedule
            .as_ref()
            .map(InstructionSchedule::enter);
        // Let siblings in CPU-first async composition register their waits.
        // Device activities were already started by connect().await.
        self.executor.yield_now().await;
        loop {
            let progress = if let Some(schedule) = &self.instruction_schedule {
                poll_fn(|cx| schedule.poll_cpu(cpu, cx)).await
            } else {
                match cpu.run().await {
                    ExecutionExit::Interrupted => RunProgress::Yield,
                    exit => RunProgress::Exit(exit),
                }
            };
            match progress {
                RunProgress::Yield => self.executor.yield_now().await,
                RunProgress::Exit(exit) => {
                    if let Some(schedule) = &self.instruction_schedule {
                        schedule.timeline.notify_due();
                        self.executor.yield_now().await;
                    }
                    return exit;
                }
            }
        }
    }

    /// Enables fixed-rate instruction scheduling without host-time pacing.
    /// CPU creation binds the clock automatically. Runs stop at the earlier
    /// quantum boundary or device deadline, allowing checkpoint overshoot.
    /// Pending CPU services advance idle time to the next registered wait;
    /// time pauses between calls to [`Self::run_cpu`]. `quantum` must be positive.
    pub fn instruction_driven(
        instructions_per_second: NonZeroU64,
        quantum: Duration,
        storage: CpuStorage,
        endpoint: WakeEndpoint<ExecutionInterrupt>,
    ) -> Self {
        let timeline = InstructionTimeline::new(crate::Instant::now(), instructions_per_second);
        let schedule = InstructionSchedule::new(timeline.clone(), quantum);
        let mut runner = Self::new(timeline.clock(), storage, endpoint);
        runner.instruction_schedule = Some(schedule);
        runner
    }
}
