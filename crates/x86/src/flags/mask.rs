use super::{Flag, StatusFlag};

/// A set of logical flags. These bits are unrelated to CPU backing offsets.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct FlagMask(u8);

impl FlagMask {
    pub(crate) const EMPTY: Self = Self(0);
    pub(crate) const STATUS: Self = Self((1 << StatusFlag::ALL.len()) - 1);
    pub(crate) const ALL: Self = Self((1 << Flag::ALL.len()) - 1);

    pub(crate) fn of(flag: impl Into<Flag>) -> Self {
        Self(1 << flag.into().index())
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) fn contains(self, flag: impl Into<Flag>) -> bool {
        self.intersects(Self::of(flag))
    }

    pub(crate) fn flags(self) -> impl Iterator<Item = Flag> {
        Flag::ALL
            .into_iter()
            .filter(move |flag| self.contains(*flag))
    }

    pub(crate) fn status_flags(self) -> impl Iterator<Item = StatusFlag> {
        StatusFlag::ALL
            .into_iter()
            .filter(move |flag| self.contains(*flag))
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub(crate) const fn covers(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn without(self, flag: impl Into<Flag>) -> Self {
        Self(self.0 & !Self::of(flag).0)
    }
}
