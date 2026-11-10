//! Fixed clock units and duration conversion, independent of clock progress.

use super::Timestamp;
use std::{num::NonZeroU64, time::Duration};

#[derive(Clone, Copy)]
pub(super) struct Timebase(pub(super) NonZeroU64);

impl Timebase {
    pub(super) const NANOSECONDS: Self = Self(NonZeroU64::new(1).unwrap());

    pub(super) fn floor(self, time: Timestamp) -> Timestamp {
        Timestamp(time.0 - time.0 % u128::from(self.0.get()))
    }

    fn ticks(self, duration: Duration) -> Option<u128> {
        duration.as_nanos().checked_mul(u128::from(self.0.get()))
    }

    pub(super) fn checked_add(self, time: Timestamp, duration: Duration) -> Option<Timestamp> {
        time.0.checked_add(self.ticks(duration)?).map(Timestamp)
    }

    pub(super) fn checked_sub(self, time: Timestamp, duration: Duration) -> Option<Timestamp> {
        time.0.checked_sub(self.ticks(duration)?).map(Timestamp)
    }

    pub(super) fn duration_since(self, time: Timestamp, earlier: Timestamp) -> Duration {
        let rate = u128::from(self.0.get());
        // Project both endpoints before subtracting: adjacent intervals must
        // account for every nanosecond even when individual ticks are finer.
        let nanos = (time.0 / rate).saturating_sub(earlier.0 / rate);
        Duration::new(
            (nanos / 1_000_000_000)
                .try_into()
                .expect("clock duration fits in u64 seconds"),
            (nanos % 1_000_000_000) as u32,
        )
    }
}
