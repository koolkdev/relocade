//! CPU execution and registered device activities on one owner.

#[cfg(feature = "worker")]
mod worker;
#[cfg(feature = "worker")]
pub use worker::ExecutionWorker;

use crate::{Clock, Cpu, Device, DeviceConnection, TimerHandle};
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
        }
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
    pub async fn run_cpu(&self, cpu: &mut Cpu) -> ExecutionExit {
        #[cfg(feature = "worker")]
        if let Some(wake) = &self.wake {
            wake.activate(cpu.interrupt_handle())
                .await
                .expect("execution host must remain alive for its session");
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
