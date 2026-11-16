use crate::Timestamp;
use std::{num::NonZeroU64, time::Duration};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Immutable native ticks per second. Equal rates do not imply a shared origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickRate(NonZeroU64);

impl TickRate {
    pub const NANOSECONDS: Self = Self(NonZeroU64::new(1_000_000_000).unwrap());

    pub const fn new(ticks_per_second: NonZeroU64) -> Self {
        Self(ticks_per_second)
    }

    pub const fn ticks_per_second(self) -> NonZeroU64 {
        self.0
    }

    pub fn deadline_after(self, start: Timestamp, delay: Duration) -> Timestamp {
        self.checked_deadline_after(start, delay)
            .expect("deadline fits in half the tick counter range")
    }

    pub fn checked_deadline_after(self, start: Timestamp, delay: Duration) -> Option<Timestamp> {
        let ticks = delay
            .as_nanos()
            .checked_mul(u128::from(self.0.get()))?
            .div_ceil(NANOS_PER_SECOND);
        (ticks <= i64::MAX as u128)
            .then(|| Timestamp::from_ticks(start.ticks().wrapping_add(ticks as u64)))
    }

    /// Subtracts a duration, rounding the resulting deadline toward the future.
    pub fn deadline_before(self, end: Timestamp, delay: Duration) -> Timestamp {
        self.checked_deadline_before(end, delay)
            .expect("deadline fits in half the tick counter range")
    }

    pub fn checked_deadline_before(self, end: Timestamp, delay: Duration) -> Option<Timestamp> {
        let numerator = delay.as_nanos().checked_mul(u128::from(self.0.get()))?;
        (numerator.div_ceil(NANOS_PER_SECOND) <= i64::MAX as u128).then(|| {
            Timestamp::from_ticks(
                end.ticks()
                    .wrapping_sub((numerator / NANOS_PER_SECOND) as u64),
            )
        })
    }

    /// Whole nanoseconds between two readings on the same clock; zero if reversed.
    /// Device cycle tracking retains its own division remainder across updates.
    pub fn duration_between(self, start: Timestamp, end: Timestamp) -> Duration {
        self.checked_duration_between(start, end)
            .unwrap_or_default()
    }

    pub fn checked_duration_between(self, start: Timestamp, end: Timestamp) -> Option<Duration> {
        (end >= start).then(|| {
            duration(
                u128::from(end.ticks().wrapping_sub(start.ticks())) * NANOS_PER_SECOND
                    / u128::from(self.0.get()),
            )
        })
    }

    /// Rounds up when translating a deadline onto another clock.
    pub fn duration_between_ceil(self, start: Timestamp, end: Timestamp) -> Duration {
        if end < start {
            return Duration::ZERO;
        }
        duration(
            (u128::from(end.ticks().wrapping_sub(start.ticks())) * NANOS_PER_SECOND)
                .div_ceil(u128::from(self.0.get())),
        )
    }
}

fn duration(nanos: u128) -> Duration {
    Duration::new(
        (nanos / NANOS_PER_SECOND)
            .try_into()
            .expect("duration fits in u64 seconds"),
        (nanos % NANOS_PER_SECOND) as u32,
    )
}
