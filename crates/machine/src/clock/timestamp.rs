/// An opaque position on one clock's timeline.
///
/// Readings from the same clock (including its clones) can be compared directly.
/// Use that clock for duration arithmetic and waits. Coordinates from different
/// clocks are unrelated, even if their numerical values happen to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Timestamp(pub(super) u128);

impl Timestamp {
    /// Constructs a reading in a [`super::ClockSource`]'s native units.
    /// Clock consumers obtain readings from [`super::Clock::now`] instead.
    pub const fn from_ticks(ticks: u128) -> Self {
        Self(ticks)
    }

    /// Returns native units for clock-source implementations. Consumers use
    /// their clock's duration operations instead of interpreting these units.
    pub const fn ticks(self) -> u128 {
        self.0
    }
}
