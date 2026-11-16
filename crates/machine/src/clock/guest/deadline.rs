use crate::{TickRate, Timestamp};
use std::time::Duration;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// A captured wait before native-tick rounding. Host lag recovery subtracts
/// durations from this value, retaining the remainder across repeated recovery.
/// Ordinary device waits start with a zero fraction.
#[derive(Clone, Copy)]
pub(super) struct MappedDeadline {
    time: Timestamp,
    fraction: u32,
}

impl From<Timestamp> for MappedDeadline {
    fn from(time: Timestamp) -> Self {
        Self { time, fraction: 0 }
    }
}

impl MappedDeadline {
    pub(super) fn after(start: Timestamp, rate: TickRate, delay: Duration) -> Option<Self> {
        let ticks = fractional_ticks(rate, delay)?;
        Some(Self {
            time: Timestamp::from_ticks(
                start
                    .ticks()
                    .wrapping_add((ticks / NANOS_PER_SECOND) as u64),
            ),
            fraction: (ticks % NANOS_PER_SECOND) as u32,
        })
    }

    pub(super) fn subtract(self, rate: TickRate, delay: Duration) -> Option<Self> {
        let ticks = fractional_ticks(rate, delay)?;
        let whole = (ticks / NANOS_PER_SECOND) as u64;
        let fraction = (ticks % NANOS_PER_SECOND) as u32;
        let borrow = u64::from(self.fraction < fraction);
        Some(Self {
            time: Timestamp::from_ticks(self.time.ticks().wrapping_sub(whole).wrapping_sub(borrow)),
            fraction: (u64::from(self.fraction) + borrow * NANOS_PER_SECOND as u64
                - u64::from(fraction)) as u32,
        })
    }

    pub(super) fn deadline(self) -> Timestamp {
        Timestamp::from_ticks(
            self.time
                .ticks()
                .wrapping_add(u64::from(self.fraction != 0)),
        )
    }
}

fn fractional_ticks(rate: TickRate, delay: Duration) -> Option<u128> {
    let ticks = delay
        .as_nanos()
        .checked_mul(u128::from(rate.ticks_per_second().get()))?;
    (ticks.div_ceil(NANOS_PER_SECOND) <= i64::MAX as u128).then_some(ticks)
}
