//! Native clock readings. Conversion belongs to duration and host-time boundaries.

use super::Instant;
use std::{
    cmp::Ordering,
    ops::{Add, Sub},
    time::Duration,
};

/// A reading or deadline in one clock's timebase.
///
/// Guest readings retain instruction progress without converting it to time.
/// Compare readings from the same clock, just as with its `Instant` readings.
/// Duration arithmetic rounds readings down to nanoseconds, matching
/// [`super::Clock::now`]. Convert to [`Instant`] when scheduling a wait or
/// exchanging a timestamp with a host-time consumer.
#[derive(Clone, Copy, Debug)]
pub struct Timestamp(Repr);

#[derive(Clone, Copy, Debug)]
enum Repr {
    Host(Instant),
    Guest {
        origin: Instant,
        ticks: u128,
        rate: u64,
    },
}

impl Timestamp {
    pub(super) fn guest(origin: Instant, ticks: u128, rate: u64) -> Self {
        Self(Repr::Guest {
            origin,
            ticks,
            rate,
        })
    }

    /// Materializes this reading in the clock's existing `Instant` timebase.
    pub fn to_instant(self) -> Instant {
        match self.0 {
            Repr::Host(time) => time,
            Repr::Guest {
                origin,
                ticks,
                rate,
            } => origin + duration(ticks / u128::from(rate)),
        }
    }

    pub fn duration_since(self, earlier: Self) -> Duration {
        self.to_instant().duration_since(earlier.to_instant())
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        Some(self.at(self.to_instant().checked_add(duration)?))
    }

    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        Some(self.at(self.to_instant().checked_sub(duration)?))
    }

    fn at(self, time: Instant) -> Self {
        match self.0 {
            Repr::Host(_) => Self::from(time),
            Repr::Guest { origin, rate, .. } => {
                match time
                    .checked_duration_since(origin)
                    .and_then(|elapsed| elapsed.as_nanos().checked_mul(u128::from(rate)))
                {
                    Some(ticks) => Self::guest(origin, ticks, rate),
                    None => Self::guest(time, 0, rate),
                }
            }
        }
    }

    fn fraction(self) -> (u64, u64) {
        match self.0 {
            Repr::Host(_) => (0, 1),
            Repr::Guest { ticks, rate, .. } => ((ticks % u128::from(rate)) as u64, rate),
        }
    }
}

impl From<Instant> for Timestamp {
    fn from(time: Instant) -> Self {
        Self(Repr::Host(time))
    }
}

impl PartialEq for Timestamp {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Timestamp {}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0, other.0) {
            (Repr::Host(a), Repr::Host(b)) => a.cmp(&b),
            (
                Repr::Guest {
                    origin: a_origin,
                    ticks: a,
                    rate: a_rate,
                },
                Repr::Guest {
                    origin: b_origin,
                    ticks: b,
                    rate: b_rate,
                },
            ) if a_origin == b_origin && a_rate == b_rate => a.cmp(&b),
            _ => self.to_instant().cmp(&other.to_instant()).then_with(|| {
                let (a, a_rate) = self.fraction();
                let (b, b_rate) = other.fraction();
                (u128::from(a) * u128::from(b_rate)).cmp(&(u128::from(b) * u128::from(a_rate)))
            }),
        }
    }
}

impl Add<Duration> for Timestamp {
    type Output = Self;

    fn add(self, duration: Duration) -> Self {
        self.checked_add(duration)
            .expect("clock timestamp overflow")
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self {
        self.checked_sub(duration)
            .expect("clock timestamp underflow")
    }
}

fn duration(nanos: u128) -> Duration {
    Duration::new(
        u64::try_from(nanos / 1_000_000_000).expect("clock duration overflow"),
        (nanos % 1_000_000_000) as u32,
    )
}
