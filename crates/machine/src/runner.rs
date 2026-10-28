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
/// All mutable execution state stays on the caller's thread. CPU quanta provide
/// cooperation, not preemption or an exact elapsed-time bound.
pub struct Runner<C = TimerHandle> {
    clock: C,
    /// Requested CPU budget per quantum; checks remain at block boundaries.
    pub instruction_quantum: NonZeroU64,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
}

impl<C: Clock> Runner<C> {
    pub fn with_clock(clock: C) -> Self {
        let (activities, pending) = mpsc::unbounded();
        Self {
            clock,
            instruction_quantum: NonZeroU64::new(10_000).unwrap(),
            activities,
            pending: Some(pending),
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
        match select(pin!(main(&self)), pin!(devices)).await {
            Either::Left((result, _)) => result,
            Either::Right(_) => unreachable!("runner retains the activity sender"),
        }
    }

    /// Runs finite CPU quanta until the first non-budget exit.
    /// A pending service retains its CPU future and original instruction budget.
    pub async fn run_cpu<T: 'static>(&self, cpu: &mut Cpu<T>) -> ExecutionExit {
        loop {
            let exit = cpu.run(self.instruction_quantum.get()).await;
            if exit != ExecutionExit::BudgetExhausted {
                return exit;
            }
            yield_now().await;
        }
    }
}
