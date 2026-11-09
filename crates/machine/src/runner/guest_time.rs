//! Configuration for the execution owner's emulated time.

use crate::Clock;
use std::{num::NonZeroU64, time::Duration};

/// Selects retirement conversion, execution slices and optional host pacing.
/// Construct this on the execution owner when it contains a host clock.
#[derive(Clone)]
pub struct GuestTimeConfig {
    pub(super) instructions_per_second: NonZeroU64,
    pub(super) slice_duration: Option<Duration>,
    pub(super) host_clock: Option<Clock>,
}

impl GuestTimeConfig {
    /// Starts unpaced, with no periodic yields beyond device deadlines.
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self {
            instructions_per_second,
            slice_duration: None,
            host_clock: None,
        }
    }

    /// Bounds each execution slice in guest time. A slice gives ready activities
    /// a turn and, with a host clock, waits for host time before continuing.
    /// Requires a positive duration; checkpoints can overshoot the boundary.
    pub fn with_slice_duration(mut self, duration: Duration) -> Self {
        assert!(
            !duration.is_zero(),
            "guest time scheduling needs a positive slice duration"
        );
        self.slice_duration = Some(duration);
        self
    }

    /// Paces active execution and idle advancement at the slice boundary.
    /// Defaults to 4 ms slices unless a duration was explicitly configured.
    /// Device deadlines subdivide each slice without requiring a host wait.
    /// The embedding must retain and drive this clock's timer service.
    pub fn with_host_clock(mut self, clock: Clock) -> Self {
        self.slice_duration.get_or_insert(Duration::from_millis(4));
        self.host_clock = Some(clock);
        self
    }
}
