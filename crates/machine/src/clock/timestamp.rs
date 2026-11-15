/// An opaque position on one clock's timeline.
///
/// Readings from the same clock (including its clones) can be compared directly
/// within half the wrapping counter's range (at most `i64::MAX` ticks apart).
/// Use that clock for duration arithmetic and waits. Coordinates from different
/// clocks are unrelated, even if their numerical values happen to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Timestamp(pub(super) u64);

impl Timestamp {
    /// Constructs a reading in a [`super::ClockSource`]'s native units.
    /// Clock consumers obtain readings from [`super::Clock::now`] instead.
    pub const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    /// Returns native units for clock-source implementations. Consumers use
    /// their clock's duration operations instead of interpreting these units.
    pub const fn ticks(self) -> u64 {
        self.0
    }
}

impl Ord for Timestamp {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0.wrapping_sub(other.0) as i64).cmp(&0)
    }
}

impl PartialOrd for Timestamp {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
