use super::{Clock, Timestamp};
use std::time::Duration;

/// A device's fractional position on a clock, used for scheduling and integration.
///
/// Advance a retained cursor to preserve periodic phase. Only round to a native
/// tick when arming a deadline; clock readings themselves carry no fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClockCursor {
    pub(super) time: Timestamp,
    // Billionths of a tick represent an integral number of nanoseconds exactly
    // at any integral tick frequency. Duration conversion owns this precision.
    pub(super) fraction: u32,
}

impl From<Timestamp> for ClockCursor {
    fn from(time: Timestamp) -> Self {
        Self { time, fraction: 0 }
    }
}

impl ClockCursor {
    /// Earliest native tick at or after this position.
    pub fn deadline(self) -> Timestamp {
        Timestamp::from_ticks(
            self.time
                .ticks()
                .wrapping_add(u64::from(self.fraction != 0)),
        )
    }

    /// Advances the ideal position without rounding to an instruction tick.
    pub fn advance(&mut self, clock: &Clock, duration: Duration) {
        *self = clock
            .timebase
            .checked_advance(*self, duration)
            .expect("clock interval fits in half the counter range");
    }
}
