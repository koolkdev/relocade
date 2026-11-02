//! CPU execution and registered device activities on one owner.

mod host;

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
use std::{
    cell::RefCell, future::Future, num::NonZeroU64, ops::RangeInclusive, pin::pin, rc::Rc,
    time::Duration,
};

/// Drives a main execution activity alongside registered device futures.
/// All mutable execution state stays on the caller's thread. CPU quanta provide
/// cooperation, not preemption or an exact elapsed-time bound.
pub struct Runner<C = TimerHandle> {
    clock: C,
    /// Requested CPU budget per quantum; checks remain at block boundaries.
    pub instruction_quantum: NonZeroU64,
    /// CPU/device work between host event-loop opportunities while a posted
    /// resource is pending. Quanta and host scheduling can overshoot this gap.
    pub host_poll_interval: Duration,
    host: host::HostEvents,
    activities: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
    pending: Option<mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>>,
}

impl<C: Clock> Runner<C> {
    pub fn with_clock(clock: C) -> Self {
        let (activities, pending) = mpsc::unbounded();
        Self {
            clock,
            instruction_quantum: NonZeroU64::new(10_000).unwrap(),
            host_poll_interval: Duration::from_millis(1),
            host: host::HostEvents::default(),
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
        let main = pin!(main(&self));
        let devices = pin!(devices);
        let activities = select(main, devices);
        match self.host.run(self.host_poll_interval, activities).await {
            Either::Left((result, _)) => result,
            Either::Right(_) => unreachable!("runner retains the activity sender"),
        }
    }

    /// Awaits a host operation while allowing CPU and device activities to
    /// continue. For module delivery, call after Rust readiness says it was
    /// posted; local asynchronous instantiation can also need host turns.
    /// Only this wait enables periodic host turns; completion or cancellation
    /// removes it. Drive all continuously runnable siblings inside `run`.
    pub async fn wait_for_host<F: Future>(&self, result: F) -> F::Output {
        self.host.wait(result).await
    }

    /// Runs finite CPU quanta until the first non-budget exit.
    /// A pending service retains its CPU future and original instruction budget.
    pub async fn run_cpu(&self, cpu: &mut Cpu) -> ExecutionExit {
        loop {
            let exit = cpu.run(self.instruction_quantum.get()).await;
            if exit != ExecutionExit::BudgetExhausted {
                return exit;
            }
            yield_now().await;
        }
    }
}
