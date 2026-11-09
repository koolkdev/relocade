//! Configuration for the execution owner's emulated time.

use crate::Clock;
use std::{num::NonZeroU64, time::Duration};

/// Selects retirement conversion, periodic yields and optional host pacing.
/// Construct this on the execution owner when it contains a host clock.
#[derive(Clone)]
pub struct GuestTimeConfig {
    pub(super) instructions_per_second: NonZeroU64,
    pub(super) yield_interval: Option<Duration>,
    pub(super) host_clock: Option<Clock>,
    pub(super) burst_interval: Duration,
}

impl GuestTimeConfig {
    /// Starts unpaced, with no periodic yields beyond device deadlines.
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self {
            instructions_per_second,
            yield_interval: None,
            host_clock: None,
            burst_interval: Duration::from_millis(4),
        }
    }

    /// Adds periodic execution yields measured in guest time.
    pub fn with_yield_interval(mut self, interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "guest time scheduling needs a positive yield interval"
        );
        self.yield_interval = Some(interval);
        self
    }

    /// Paces active execution and idle advancement in 4 ms guest-time bursts.
    /// Device deadlines subdivide each burst without requiring a host wait.
    /// The embedding must retain and drive this clock's timer service.
    pub fn with_host_clock(mut self, clock: Clock) -> Self {
        self.host_clock = Some(clock);
        self
    }

    /// Changes the guest-time allowance between host pacing waits.
    /// Requires a positive duration; has no effect without a host clock.
    pub fn with_burst_interval(mut self, interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "guest pacing needs a positive burst interval"
        );
        self.burst_interval = interval;
        self
    }
}
