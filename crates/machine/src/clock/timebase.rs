//! Fixed clock units and duration conversion, independent of clock progress.

use super::Timestamp;
use std::{num::NonZeroU64, time::Duration};

#[derive(Clone, Copy)]
pub(super) struct Timebase(pub(super) NonZeroU64);

impl Timebase {
    pub(super) const NANOSECONDS: Self = Self(NonZeroU64::new(1).unwrap());

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
        duration(time.0.saturating_sub(earlier.0) / u128::from(self.0.get()))
    }

    pub(super) fn consume_elapsed(self, cursor: &mut Timestamp, time: Timestamp) -> Duration {
        let elapsed = self.duration_since(time, *cursor);
        // Advancing by the duration returned leaves any unconsumed native
        // fraction between the cursor and the observation for the next update.
        *cursor = self
            .checked_add(*cursor, elapsed)
            .expect("consumed time fits before observation");
        elapsed
    }
}

fn duration(nanos: u128) -> Duration {
    Duration::new(
        (nanos / 1_000_000_000)
            .try_into()
            .expect("clock duration fits in u64 seconds"),
        (nanos % 1_000_000_000) as u32,
    )
}
