//! Clonable timebases and owned deadline waits.

mod driver;
mod host;
mod instruction;
pub use instruction::InstructionTimeline;
mod origin;
pub use driver::HostTimerDriver;
pub use host::{timer_service, TimerHandle};
pub use origin::TimeOrigin;
pub use web_time::{Instant, SystemTime};

use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};

/// Read and wait on one timebase. Clones share its origin and progress.
/// A clock stays on its execution owner; its controller supplies the operations
/// that advance time or drive timers. Host and guest clocks are chosen at setup.
#[derive(Clone)]
pub struct Clock(Rc<dyn ClockSource>);

/// Supplies monotonic readings and cancellable waits in one timebase.
/// Implementations register waits on first poll and release them on drop.
/// Sources stay on their execution owner; controllers may have broader lifetimes.
pub trait ClockSource {
    fn now(&self) -> Instant;
    fn sleep_until(&self, deadline: Instant) -> Sleep;
}

impl Clock {
    /// Wraps a source behind the common consumer API.
    pub fn new(source: impl ClockSource + 'static) -> Self {
        Self(Rc::new(source))
    }

    pub fn now(&self) -> Instant {
        self.0.now()
    }

    /// Captures the deadline now, even if the returned future is polled later.
    pub fn sleep(&self, duration: Duration) -> Sleep {
        self.sleep_until(self.now() + duration)
    }

    /// Waits on a timestamp from this clock's timebase. Registration happens on
    /// first poll; dropping the future cancels the wait. An expired deadline may
    /// complete immediately, so awaiting a sleep does not promise an executor yield.
    pub fn sleep_until(&self, deadline: Instant) -> Sleep {
        self.0.sleep_until(deadline)
    }
}

/// An owned deadline wait that does not borrow its clock or a device.
#[must_use = "a sleep must be polled to register its deadline"]
pub struct Sleep(Pin<Box<dyn Future<Output = ()>>>);

impl Sleep {
    /// Owns a source future, including its registration and cancellation state.
    pub fn new(future: impl Future<Output = ()> + 'static) -> Self {
        Self(Box::pin(future))
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.as_mut().poll(cx)
    }
}
