//! Complete replacements and partial changes to status flags.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::{FlagSource, LocalFlagSource, StatusFlag};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct FlagMask(u8);

impl FlagMask {
    pub(crate) const EMPTY: Self = Self(0);
    pub(crate) const ALL: Self = Self((1 << StatusFlag::ALL.len()) - 1);

    pub(crate) const fn of(flag: StatusFlag) -> Self {
        Self(1 << flag as u8)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) const fn contains(self, flag: StatusFlag) -> bool {
        self.intersects(Self::of(flag))
    }

    pub(crate) fn flags(self) -> impl Iterator<Item = StatusFlag> {
        StatusFlag::ALL
            .into_iter()
            .filter(move |flag| self.contains(*flag))
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

pub(crate) enum FlagChange {
    Complete(LocalFlagSource),
    Partial([Option<Val<I1>>; 6]),
}

impl FlagChange {
    /// Omitted flags retain their previous values.
    pub(crate) fn partial<const N: usize>(flags: [(StatusFlag, Val<I1>); N]) -> Self {
        let mut values = StatusFlag::ALL.map(|_| None);
        for (flag, value) in flags {
            values[flag as usize] = Some(value);
        }
        Self::Partial(values)
    }

    pub(crate) fn writes(&self) -> FlagMask {
        match self {
            Self::Complete(_) => FlagMask::ALL,
            Self::Partial(flags) => StatusFlag::ALL.iter().fold(FlagMask::EMPTY, |mask, flag| {
                if flags[*flag as usize].is_some() {
                    mask.union(FlagMask::of(*flag))
                } else {
                    mask
                }
            }),
        }
    }

    pub(crate) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match self {
            Self::Complete(source) => source.flag(flag),
            Self::Partial(flags) => flags[flag as usize]
                .as_ref()
                .expect("the change defines the requested flag")
                .clone(),
        }
    }
}

impl<T: MemoryInt> FlagSource<T> {
    /// Applies this source while preserving one flag from the current state.
    pub(crate) fn preserving(self, flag: StatusFlag) -> FlagChange {
        FlagChange::Partial(
            StatusFlag::ALL.map(|candidate| (candidate != flag).then(|| self.flag(candidate))),
        )
    }
}

impl<T: MemoryInt> From<FlagSource<T>> for FlagChange
where
    FlagSource<T>: Into<LocalFlagSource>,
{
    fn from(source: FlagSource<T>) -> Self {
        Self::Complete(source.into())
    }
}
