//! Clonable timebases and owned deadline waits.

mod driver;
mod guest;
mod host;
pub use driver::HostTimerDriver;
pub use guest::GuestTimeDriver;
pub use host::{timer_service, TimerHandle};
pub use web_time::Instant;

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

    /// Converts a source timestamp to the configured host timebase. Sources
    /// without an active correspondence return `None`.
    fn host_time(&self, _time: Instant) -> Option<Instant> {
        None
    }

    /// Captures a host-to-source deadline and returns an ordinary source wait.
    /// `None` samples the configured host clock now; `Some` uses a timestamp
    /// from that host timebase. Sources without a host mapping complete immediately.
    fn sync(&self, _host_time: Option<Instant>) -> Sleep {
        Sleep::new(async {})
    }
}

impl Clock {
    /// Wraps a source behind the common consumer API.
    pub fn new(source: impl ClockSource + 'static) -> Self {
        Self(Rc::new(source))
    }

    pub fn now(&self) -> Instant {
        self.0.now()
    }

    /// Captures the host time corresponding to a timestamp from this clock.
    /// Output producers can send the result to another worker and schedule
    /// presentation there without sharing this execution-local clock.
    ///
    /// Host clocks return the timestamp unchanged. Paced guest clocks use the
    /// current correspondence, excluding pauses between CPU runs. Unpaced guest
    /// clocks and clocks outside an active run return `None`. Convert when the
    /// output is captured: a later run may have a different correspondence.
    /// The returned timestamp stays fixed, including across pauses.
    pub fn host_time(&self, time: Instant) -> Option<Instant> {
        self.0.host_time(time)
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

    /// Waits until guest time reaches the host's current mapped time, captured
    /// now. A guest already ahead completes immediately. This is a guest timer,
    /// not a host pacing wait; later host progress does not extend its deadline.
    /// Host clocks, unpaced guest clocks and clocks outside a CPU run do not wait.
    pub fn sync(&self) -> Sleep {
        self.0.sync(None)
    }

    /// Like [`Self::sync`], using a timestamp from the configured host clock's
    /// timebase, such as the time an input event was enqueued. The current
    /// host/guest correspondence is captured now and retained by the wait.
    /// It does not reconstruct past mappings: input first consumed after a pause
    /// uses the resumed correspondence, while an already-created wait is unchanged.
    pub fn sync_to(&self, host_time: Instant) -> Sleep {
        self.0.sync(Some(host_time))
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
