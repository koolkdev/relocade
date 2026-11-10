//! Clonable timebases and owned deadline waits.

mod driver;
mod guest;
mod host;
mod timebase;
mod timestamp;
pub use driver::HostTimerDriver;
pub use guest::GuestTimeDriver;
pub use host::{timer_service, TimerHandle};
use timebase::Timebase;
pub use timestamp::Timestamp;
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
pub struct Clock {
    source: Rc<dyn ClockSource>,
    timebase: Timebase,
}

/// Supplies monotonic readings and cancellable waits in one timebase.
/// Implementations register waits on first poll and release them on drop.
/// Clocks and their waits stay on their execution owner.
pub trait ClockSource {
    fn now(&self) -> Timestamp;

    fn sleep_until(&self, deadline: Timestamp) -> Sleep;

    /// Maps an external host event into this source's timeline. Host sources
    /// clamp events before their origin to the origin. Sources without a host
    /// correspondence leave the default; their input synchronization is immediate.
    fn host_timestamp(&self, _time: Instant) -> Option<Timestamp> {
        None
    }

    /// Captures a local wait on the configured host timebase. Sources without
    /// an active correspondence complete immediately.
    fn sync_to_host(&self, _time: Timestamp) -> Sleep {
        Sleep::new(async {})
    }

    /// Captures a host-to-source deadline. Guest lag recovery may shorten it.
    /// `None` samples the configured host clock now; `Some` supplies an external
    /// host `Instant`, translated through the host source's `host_timestamp`.
    /// Sources without a host mapping complete immediately.
    fn sync(&self, _host_time: Option<Instant>) -> Sleep {
        Sleep::new(async {})
    }
}

impl Clock {
    /// Wraps a source behind the common consumer API.
    pub fn new(source: impl ClockSource + 'static) -> Self {
        Self {
            timebase: Timebase::NANOSECONDS,
            source: Rc::new(source),
        }
    }

    /// Reads native progress on this clock's timeline.
    pub fn now(&self) -> Timestamp {
        self.source.now()
    }

    /// Adds an exact duration without discarding fractional clock progress.
    /// Panics if the result exceeds the clock's coordinate range.
    pub fn add(&self, time: Timestamp, duration: Duration) -> Timestamp {
        self.checked_add(time, duration)
            .expect("clock timestamp overflow")
    }

    pub fn checked_add(&self, time: Timestamp, duration: Duration) -> Option<Timestamp> {
        self.timebase.checked_add(time, duration)
    }

    /// Subtracts an exact duration. Panics if the result precedes the origin.
    pub fn sub(&self, time: Timestamp, duration: Duration) -> Timestamp {
        self.checked_sub(time, duration)
            .expect("clock timestamp underflow")
    }

    pub fn checked_sub(&self, time: Timestamp, duration: Duration) -> Option<Timestamp> {
        self.timebase.checked_sub(time, duration)
    }

    /// Actual elapsed time, rounded down to whole nanoseconds. Returns zero
    /// when `earlier` is later than `time`. Both readings must belong to this clock.
    /// Repeated integration uses [`Self::consume_elapsed`] to retain fractions.
    pub fn duration_since(&self, time: Timestamp, earlier: Timestamp) -> Duration {
        self.timebase.duration_since(time, earlier)
    }

    pub fn checked_duration_since(&self, time: Timestamp, earlier: Timestamp) -> Option<Duration> {
        (time >= earlier).then(|| self.duration_since(time, earlier))
    }

    /// Consumes whole nanoseconds through `time`, advancing `cursor` only by
    /// the returned duration. A fractional remainder stays behind for the next
    /// update. Earlier observations leave the cursor unchanged.
    ///
    /// Initialize the cursor from `now()` when the activity starts. Device state
    /// and its next deadline describe this consumed position, not the later raw
    /// observation. This keeps repeated updates independent of observation frequency.
    pub fn consume_elapsed(&self, cursor: &mut Timestamp, time: Timestamp) -> Duration {
        self.timebase.consume_elapsed(cursor, time)
    }

    /// Waits for the host time corresponding to a timestamp from this clock.
    /// CPU pacing and device output use the configured host timer on this owner.
    /// Output tasks await this before publishing to another worker.
    ///
    /// Host clocks use the timestamp unchanged. Paced guest clocks use the
    /// current correspondence, excluding pauses between CPU runs. Unpaced guest
    /// clocks and clocks outside an active run complete immediately. The mapping
    /// is captured when this method is called. The deadline stays fixed, including
    /// across pauses and guest lag recovery.
    /// Registration happens on first poll; elapsed deadlines need no timer.
    pub fn sync_to_host(&self, time: Timestamp) -> Sleep {
        self.source.sync_to_host(time)
    }

    fn sleep_until_if_needed(&self, deadline: Timestamp) -> Sleep {
        let clock = self.clone();
        Sleep::new(async move {
            if clock.now() < deadline {
                clock.sleep_until(deadline).await;
            }
        })
    }

    /// Captures the deadline now, even if the returned future is polled later.
    pub fn sleep(&self, duration: Duration) -> Sleep {
        self.sleep_until(self.add(self.now(), duration))
    }

    /// Waits on a timestamp from this clock's timebase. Registration happens on
    /// first poll; dropping the future cancels the wait. An expired deadline may
    /// complete immediately, so awaiting a sleep does not promise an executor yield.
    pub fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        self.source.sleep_until(deadline)
    }

    /// Waits until guest time reaches the host's current mapped time, captured
    /// now. A guest already ahead completes immediately. This is a guest timer,
    /// not a host pacing wait; later host progress does not extend its deadline.
    /// Discarding excessive guest lag shortens this wait with the shared mapping.
    /// Host clocks, unpaced guest clocks and clocks outside a CPU run do not wait.
    pub fn sync(&self) -> Sleep {
        self.source.sync(None)
    }

    /// Like [`Self::sync`], using the external host instant at which an input
    /// event was enqueued. The current host/guest correspondence is captured now. Guest lag recovery shortens
    /// pending waits, including ones that have not yet been polled.
    /// It does not reconstruct past mappings: input first consumed after a pause
    /// uses the resumed correspondence, while an ordinary pause leaves an
    /// already-created wait unchanged.
    pub fn sync_to(&self, host_time: Instant) -> Sleep {
        self.source.sync(Some(host_time))
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
