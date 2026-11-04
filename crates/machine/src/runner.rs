//! CPU execution and registered device activities on one owner.

mod worker;
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
    session: Option<Rc<crate::worker::SessionDriver<crate::ExecutionInterrupt>>>,
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
            session: None,
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
        if let Some(session) = &self.session {
            let storage = self
                .storage
                .borrow_mut()
                .take()
                .expect("session CPU storage already consumed");
            let mut cpu = storage.into_cpu(memory, config, io).await?;
            cpu.session = Rc::downgrade(session);
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
        // Poll ready devices before returning to guest execution.
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
        if let Some(session) = &self.session {
            assert!(
                std::ptr::eq(cpu.session.as_ptr(), Rc::as_ptr(session)),
                "construct the session CPU with runner.create_cpu"
            );
            // Give devices registered during application setup a polling
            // opportunity before the first synchronous guest interval.
            session.yield_now().await;
        }
        let quantum = self.instruction_quantum.get();
        let mut remaining = quantum;
        let state = cpu.cpu_state();
        let interrupt = cpu.interrupt_handle();
        loop {
            if let Some(session) = &self.session {
                if interrupt.is_requested() {
                    // Give already-notified work a Rust poll before entering
                    // Wasm. Do this once: nested combinators may synthesize a
                    // wake on each yield, so waiting for silence can starve CPU.
                    session.yield_now().await;
                }
            }
            let before = state.instruction_count();
            match cpu.run(remaining).await {
                ExecutionExit::BudgetExhausted => remaining = quantum,
                ExecutionExit::Interrupted if self.session.is_some() => {
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
        if let Some(session) = &self.session {
            session.yield_now().await;
            return;
        }
        yield_now().await;
    }
}
