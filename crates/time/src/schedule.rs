use crate::{TickRate, Timestamp};
use std::time::Duration;

/// Periodic timing anchored to an integer clock reading.
///
/// Durations accumulate before conversion to ticks. Deadline rounding never
/// changes the retained schedule, so fractional periods cannot accumulate drift.
/// No live clock is retained.
#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    rate: TickRate,
    anchor: Timestamp,
    elapsed: Duration,
}

impl Schedule {
    pub fn new(rate: TickRate, start: Timestamp) -> Self {
        Self {
            rate,
            anchor: start,
            elapsed: Duration::ZERO,
        }
    }

    pub fn advance(&mut self, duration: Duration) {
        self.elapsed += duration;
    }

    pub fn deadline(&self) -> Timestamp {
        self.rate.deadline_after(self.anchor, self.elapsed)
    }

    /// Whole nanoseconds of lateness, measured from the retained schedule.
    pub fn elapsed_until(&self, now: Timestamp) -> Duration {
        self.rate
            .duration_between(self.anchor, now)
            .saturating_sub(self.elapsed)
    }

    pub fn reset(&mut self, start: Timestamp) {
        self.anchor = start;
        self.elapsed = Duration::ZERO;
    }
}
