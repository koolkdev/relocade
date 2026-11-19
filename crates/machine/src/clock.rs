//! Clock readings, integer deadlines and owned waits.

mod driver;
mod guest;
mod host;

pub use driver::HostTimerDriver;
pub use guest::GuestTimeDriver;
pub use host::{timer_service, TimerHandle};
pub use relocade_time::{Schedule, TickRate, Timestamp};
pub use web_time::Instant;

use std::{
    future::Future,
    num::NonZeroU64,
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
    tick_rate: TickRate,
}

/// Supplies monotonic readings and cancellable waits in one timebase.
/// Implementations register waits on first poll and release them on drop.
/// Clocks and their waits stay on their execution owner.
pub trait ClockSource {
    fn now(&self) -> Timestamp;

    fn sleep_until(&self, deadline: Timestamp) -> Sleep;

    /// Native ticks per second. Nanosecond sources use the default; a guest
    /// instruction clock supplies its configured instruction rate.
    fn ticks_per_second(&self) -> NonZeroU64 {
        NonZeroU64::new(1_000_000_000).unwrap()
    }

    /// Captures a local wait on the configured host timebase. Sources without
    /// an active correspondence complete immediately.
    fn sync_to_host(&self, _time: Timestamp) -> Sleep {
        Sleep::new(async {})
    }
}

impl Clock {
    /// Wraps a source behind the common consumer API.
    pub fn new(source: impl ClockSource + 'static) -> Self {
        Self {
            tick_rate: TickRate::new(source.ticks_per_second()),
            source: Rc::new(source),
        }
    }

    /// Reads native progress on this clock's timeline.
    #[inline]
    pub fn now(&self) -> Timestamp {
        self.source.now()
    }

    /// Copies the immutable conversion scale; this value can cross threads.
    pub fn tick_rate(&self) -> TickRate {
        self.tick_rate
    }

    /// Calculates the first native tick at or after the requested time.
    pub fn deadline_after(&self, start: Timestamp, delay: Duration) -> Timestamp {
        self.tick_rate().deadline_after(start, delay)
    }

    pub fn checked_deadline_after(&self, start: Timestamp, delay: Duration) -> Option<Timestamp> {
        self.tick_rate().checked_deadline_after(start, delay)
    }

    pub fn deadline_before(&self, end: Timestamp, delay: Duration) -> Timestamp {
        self.tick_rate().deadline_before(end, delay)
    }

    pub fn checked_deadline_before(&self, end: Timestamp, delay: Duration) -> Option<Timestamp> {
        self.tick_rate().checked_deadline_before(end, delay)
    }

    /// Measures a single interval in whole nanoseconds.
    pub fn duration_between(&self, start: Timestamp, end: Timestamp) -> Duration {
        self.tick_rate().duration_between(start, end)
    }

    pub fn checked_duration_between(&self, start: Timestamp, end: Timestamp) -> Option<Duration> {
        self.tick_rate().checked_duration_between(start, end)
    }

    /// Retains periodic phase while publishing integer deadlines.
    pub fn schedule(&self, start: Timestamp) -> Schedule {
        Schedule::new(self.tick_rate(), start)
    }

    /// Waits for the host time corresponding to a timestamp from this clock.
    /// CPU pacing and device output use the configured host timer on this owner.
    /// Output tasks await this before publishing to another worker.
    ///
    /// Host clocks use the timestamp unchanged. Paced guest clocks use the
    /// current correspondence. A managed session keeps its mapping between CPU runs.
    /// Unpaced guest clocks and unbound clocks complete immediately. The mapping
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
        self.sleep_until(self.deadline_after(self.now(), duration))
    }

    /// Waits on a timestamp from this clock's timebase. Registration happens on
    /// first poll; dropping the future cancels the wait. An expired deadline may
    /// complete immediately, so awaiting a sleep does not promise an executor yield.
    pub fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        self.source.sleep_until(deadline)
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
