//! Native tick frequency and precise duration conversion at device boundaries.

use super::{ClockCursor, Timestamp};
use std::{num::NonZeroU64, time::Duration};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

#[derive(Clone, Copy)]
pub(super) struct Timebase(pub(super) NonZeroU64);

impl Timebase {
    pub(super) const NANOSECONDS: Self = Self(NonZeroU64::new(1_000_000_000).unwrap());

    fn fractional_ticks(self, duration: Duration) -> Option<u128> {
        let ticks = duration.as_nanos().checked_mul(u128::from(self.0.get()))?;
        (ticks.div_ceil(NANOS_PER_SECOND) <= i64::MAX as u128).then_some(ticks)
    }

    pub(super) fn checked_advance(
        self,
        cursor: ClockCursor,
        duration: Duration,
    ) -> Option<ClockCursor> {
        let ticks = self.fractional_ticks(duration)? + u128::from(cursor.fraction);
        Some(ClockCursor {
            time: Timestamp::from_ticks(
                cursor
                    .time
                    .ticks()
                    .wrapping_add((ticks / NANOS_PER_SECOND) as u64),
            ),
            fraction: (ticks % NANOS_PER_SECOND) as u32,
        })
    }

    pub(super) fn checked_add(self, time: ClockCursor, duration: Duration) -> Option<Timestamp> {
        self.checked_advance(time, duration)
            .map(ClockCursor::deadline)
    }

    pub(super) fn checked_sub(self, time: ClockCursor, duration: Duration) -> Option<Timestamp> {
        self.checked_retreat(time, duration)
            .map(ClockCursor::deadline)
    }

    pub(super) fn checked_retreat(
        self,
        time: ClockCursor,
        duration: Duration,
    ) -> Option<ClockCursor> {
        let ticks = self.fractional_ticks(duration)?;
        let whole = (ticks / NANOS_PER_SECOND) as u64;
        let fraction = (ticks % NANOS_PER_SECOND) as u32;
        let borrow = u64::from(time.fraction < fraction);
        Some(ClockCursor {
            time: Timestamp::from_ticks(time.time.ticks().wrapping_sub(whole).wrapping_sub(borrow)),
            fraction: ((u64::from(time.fraction) + borrow * NANOS_PER_SECOND as u64)
                - u64::from(fraction)) as u32,
        })
    }

    fn elapsed(self, time: ClockCursor, earlier: ClockCursor) -> u128 {
        if time < earlier {
            return 0;
        }
        u128::from(time.time.ticks().wrapping_sub(earlier.time.ticks())) * NANOS_PER_SECOND
            + u128::from(time.fraction)
            - u128::from(earlier.fraction)
    }

    pub(super) fn duration_since(self, time: ClockCursor, earlier: ClockCursor) -> Duration {
        duration(self.elapsed(time, earlier) / u128::from(self.0.get()))
    }

    /// Cross-clock deadlines must not fire before their source time.
    pub(super) fn duration_until(self, time: ClockCursor, earlier: ClockCursor) -> Duration {
        duration(
            self.elapsed(time, earlier)
                .div_ceil(u128::from(self.0.get())),
        )
    }

    pub(super) fn consume_elapsed(self, cursor: &mut ClockCursor, time: ClockCursor) -> Duration {
        let elapsed = self.duration_since(time, *cursor);
        *cursor = self
            .checked_advance(*cursor, elapsed)
            .expect("consumed time fits before observation");
        elapsed
    }
}

fn duration(nanos: u128) -> Duration {
    Duration::new(
        (nanos / NANOS_PER_SECOND)
            .try_into()
            .expect("clock duration fits in u64 seconds"),
        (nanos % NANOS_PER_SECOND) as u32,
    )
}
