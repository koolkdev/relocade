//! CPU execution and registered device activities on one owner.

#[cfg(feature = "worker")]
mod worker;
#[cfg(feature = "worker")]
pub use worker::ExecutionWorker;

use crate::{Clock, Cpu, CpuConfig, Device, DeviceConnection, IoBus, Memory, Result, TimerHandle};
use futures_channel::mpsc;
/// Gives other ready Rust activities a polling opportunity without a host timer.
/// This does not promise a browser event-loop turn.
pub use futures_lite::future::yield_now;
use futures_util::{
    future::{select, Either, LocalBoxFuture},
    StreamExt,
};
use relocade_x86::ExecutionExit;
use std::{cell::RefCell, num::NonZeroU64, ops::RangeInclusive, pin::pin, rc::Rc};

/// Drives a main execution activity alongside registered device futures.
/// All mutable execution state stays on the caller's thread. Connected workers
/// yield on notifications; instruction quanta remain a temporary fallback.
pub struct Runner<C = TimerHandle> {
    clock: C,
    /// Requested CPU budget per quantum; checks remain at block boundaries.
    pub instruction_quantum: NonZeroU64,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
    #[cfg(feature = "worker")]
    wake: Option<Rc<crate::worker::WakeConnection<crate::ExecutionInterrupt>>>,
    #[cfg(feature = "worker")]
    storage: RefCell<Option<crate::CpuStorage>>,
}

impl<C: Clock> Runner<C> {
    pub fn with_clock(clock: C) -> Self {
        let (activities, pending) = mpsc::unbounded();
        Self {
            clock,
            instruction_quantum: NonZeroU64::new(10_000).unwrap(),
            activities,
            pending: Some(pending),
            #[cfg(feature = "worker")]
            wake: None,
            #[cfg(feature = "worker")]
            storage: RefCell::default(),
        }
    }

    /// Constructs a CPU on this owner. An execution worker consumes its prepared
    /// storage once; subsequent programs reuse that CPU. Standalone runners can
    /// construct independent CPUs through the ordinary allocation path.
    ///
    /// # Panics
    /// Panics if a connected worker's storage was consumed by an earlier
    /// construction attempt, including one that failed or was cancelled.
    pub async fn create_cpu(&self, memory: Memory, config: CpuConfig, io: IoBus) -> Result<Cpu> {
        #[cfg(feature = "worker")]
        if let Some(wake) = &self.wake {
            let storage = self
                .storage
                .borrow_mut()
                .take()
                .expect("session CPU storage already consumed");
            let mut cpu = storage.into_cpu(memory, config, io).await?;
            cpu.session = Rc::downgrade(wake);
            return Ok(cpu);
        }
        Cpu::new(memory, config, io).await
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// Registers a device's activity and creates its bus connection together.
    /// The returned connection belongs in the CPU's [`crate::IoBus`].
    pub fn connect<D: Device + 'static>(
        &self,
        device: Rc<RefCell<D>>,
        mmio: impl IntoIterator<Item = RangeInclusive<u32>>,
        ports: impl IntoIterator<Item = RangeInclusive<u16>>,
    ) -> DeviceConnection {
        self.activities
            .unbounded_send(Box::pin(D::run(device.clone())))
            .unwrap_or_else(|_| panic!("runner must own its device activities"));
        DeviceConnection {
            device,
            mmio: mmio.into_iter().collect(),
            ports: ports.into_iter().collect(),
        }
    }

    /// Runs the main activity and devices for one execution lifetime.
    /// Devices remain active while the main activity awaits input or another
    /// process. Completion or cancellation drops every activity on this owner.
    pub async fn run<R>(mut self, main: impl AsyncFnOnce(&Self) -> R) -> R {
        let pending = self.pending.take().expect("new runner activity queue");
        let devices = pending.for_each_concurrent(None, |activity| activity);
        // Drain ready devices before returning to guest execution.
        match select(pin!(devices), pin!(main(&self))).await {
            Either::Right((result, _)) => result,
            Either::Left(_) => unreachable!("runner retains the activity sender"),
        }
    }

    /// Runs until the guest exits. Connected execution workers cooperate on
    /// notifications without replenishing the temporary instruction quantum.
    /// A pending service retains its CPU future and original instruction budget.
    ///
    /// # Panics
    /// A connected worker accepts only the CPU created by this runner.
    pub async fn run_cpu(&self, cpu: &mut Cpu) -> ExecutionExit {
        #[cfg(feature = "worker")]
        if let Some(wake) = &self.wake {
            assert!(
                std::ptr::eq(cpu.session.as_ptr(), Rc::as_ptr(wake)),
                "construct the session CPU with runner.create_cpu"
            );
            // Give devices registered during application setup a polling
            // opportunity before the first synchronous guest interval.
            wake.yield_now().await;
        }
        let quantum = self.instruction_quantum.get();
        let mut remaining = quantum;
        #[cfg(feature = "worker")]
        let state = cpu.cpu_state();
        loop {
            #[cfg(feature = "worker")]
            let before = state.instruction_count();
            match cpu.run(remaining).await {
                ExecutionExit::BudgetExhausted => remaining = quantum,
                #[cfg(feature = "worker")]
                ExecutionExit::Interrupted if self.wake.is_some() => {
                    let spent = state.instruction_count().wrapping_sub(before);
                    remaining = remaining.saturating_sub(spent);
                    if remaining == 0 {
                        remaining = quantum;
                    }
                }
                exit => return exit,
            }
            self.yield_execution().await;
        }
    }

    async fn yield_execution(&self) {
        #[cfg(feature = "worker")]
        if let Some(wake) = &self.wake {
            wake.yield_now().await;
            return;
        }
        yield_now().await;
    }
}
