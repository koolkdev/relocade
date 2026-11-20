//! Configuration for the execution owner's emulated time.

use std::{num::NonZeroU64, time::Duration};

/// Selects retirement conversion, execution slices and optional host pacing.
#[derive(Clone, Copy, Debug)]
pub struct GuestTimeConfig {
    pub(super) instructions_per_second: NonZeroU64,
    pub(super) slice_duration: Option<Duration>,
}

impl GuestTimeConfig {
    /// Starts unpaced, with no periodic yields beyond device deadlines.
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self {
            instructions_per_second,
            slice_duration: None,
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
}
