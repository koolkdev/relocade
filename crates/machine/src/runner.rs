//! CPU execution and registered device activities on one owner.

mod worker;
pub use worker::ExecutionWorker;

use crate::worker::{Executor, WakeEndpoint};
use crate::{
    Clock, Cpu, CpuConfig, CpuStorage, Device, DeviceConnection, ExecutionInterrupt, IoBus, Memory,
    Result, TimerHandle,
};
use futures_channel::{mpsc, oneshot};
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
};

/// Owns a worker's execution session, including its CPU storage and device activities.
/// [`ExecutionWorker`] prepares the resources before starting the owner thread.
/// Notifications interrupt guest execution; instruction quanta remain temporary.
pub struct Runner<C = TimerHandle> {
    clock: C,
    /// Requested CPU budget per quantum; checks remain at block boundaries.
    pub instruction_quantum: NonZeroU64,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    executor: Rc<Executor<ExecutionInterrupt>>,
    storage: RefCell<Option<CpuStorage>>,
}

impl<C: Clock> Runner<C> {
    /// Binds prepared CPU storage and its wake endpoint on the execution owner.
    /// Normally called by [`ExecutionWorker`]. The endpoint must come from a
    /// connection created for this storage; retain its host until the session ends.
    pub fn new(clock: C, storage: CpuStorage, endpoint: WakeEndpoint<ExecutionInterrupt>) -> Self {
        let (activities, pending) = mpsc::unbounded();
        let executor = Rc::new(Executor::new(endpoint.bind(storage.interrupt_handle())));
        Self {
            clock,
            instruction_quantum: NonZeroU64::new(10_000).unwrap(),
            activities,
            pending: Some(pending),
            executor,
            storage: RefCell::new(Some(storage)),
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
        Ok(cpu)
    }

    pub fn clock(&self) -> &C {
        &self.clock
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
        self.activities
            .unbounded_send(Box::pin(async move {
                let mut activity = pin!(D::run(activity_device));
                let mut started = Some(started);
                poll_fn(|cx| {
                    let result = activity.as_mut().poll(cx);
                    if let Some(started) = started.take() {
                        let _ = started.send(());
                    }
                    result
                })
                .await;
            }))
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

    /// Runs until the guest exits. Execution cooperates on
    /// notifications without replenishing the temporary instruction quantum.
    /// A pending service retains its CPU future and original instruction budget.
    /// Poll within [`Self::run`], which drives the session's scheduling and devices.
    ///
    /// # Panics
    /// Panics if the CPU was not created by this runner.
    pub async fn run_cpu(&self, cpu: &mut Cpu) -> ExecutionExit {
        assert!(
            std::ptr::eq(cpu.executor.as_ptr(), Rc::as_ptr(&self.executor)),
            "construct the session CPU with runner.create_cpu"
        );
        // Let siblings in CPU-first async composition register their waits.
        // Device activities were already started by connect().await.
        self.executor.yield_now().await;
        let quantum = self.instruction_quantum.get();
        let mut remaining = quantum;
        let state = cpu.cpu_state();
        loop {
            let before = state.instruction_count();
            match cpu.run(remaining).await {
                ExecutionExit::BudgetExhausted => remaining = quantum,
                ExecutionExit::Interrupted => {
                    let spent = state.instruction_count().wrapping_sub(before);
                    remaining = remaining.saturating_sub(spent);
                    if remaining == 0 {
                        remaining = quantum;
                    }
                }
                exit => return exit,
            }
            self.executor.yield_now().await;
        }
    }
}
