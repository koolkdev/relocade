//! Complete and partial flag changes, with optional conditions for applying them.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::{AnyFlagSource, FlagSource, StatusFlag};

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

pub(crate) struct FlagChange {
    /// None denotes an unconditional change.
    pub(crate) condition: Option<Val<I1>>,
    pub(crate) values: FlagValues,
}

pub(crate) enum FlagValues {
    Complete(AnyFlagSource),
    Partial([Option<Val<I1>>; 6]),
}

impl FlagChange {
    /// Omitted flags retain their previous values.
    pub(crate) fn partial<const N: usize>(flags: [(StatusFlag, Val<I1>); N]) -> Self {
        let mut values = StatusFlag::ALL.map(|_| None);
        for (flag, value) in flags {
            values[flag as usize] = Some(value);
        }
        Self {
            condition: None,
            values: FlagValues::Partial(values),
        }
    }

    /// Restricts this change to executions where the predicate is true.
    /// Repeated conditions combine with AND; they do not replace earlier ones.
    pub(crate) fn when(mut self, condition: impl Into<Val<I1>>) -> Self {
        let condition = condition.into();
        self.condition = Some(match self.condition {
            Some(previous) => previous.and(condition),
            None => condition,
        });
        self
    }

    /// Flags potentially written when this change's condition holds.
    pub(crate) fn writes(&self) -> FlagMask {
        match &self.values {
            FlagValues::Complete(_) => FlagMask::ALL,
            FlagValues::Partial(flags) => {
                StatusFlag::ALL.iter().fold(FlagMask::EMPTY, |mask, flag| {
                    if flags[*flag as usize].is_some() {
                        mask.union(FlagMask::of(*flag))
                    } else {
                        mask
                    }
                })
            }
        }
    }

    pub(crate) fn flag(&self, flag: StatusFlag) -> Val<I1> {
        match &self.values {
            FlagValues::Complete(source) => source.flag(flag),
            FlagValues::Partial(flags) => flags[flag as usize]
                .as_ref()
                .expect("the change defines the requested flag")
                .clone(),
        }
    }

    /// Leaves this flag unchanged, retaining the condition and all other changes.
    pub(crate) fn preserving(self, flag: StatusFlag) -> Self {
        let written = self.writes();
        let values = FlagValues::Partial(StatusFlag::ALL.map(|candidate| {
            (candidate != flag && written.contains(candidate)).then(|| self.flag(candidate))
        }));
        Self { values, ..self }
    }
}

impl<T: MemoryInt> From<FlagSource<T>> for FlagChange
where
    FlagSource<T>: Into<AnyFlagSource>,
{
    fn from(source: FlagSource<T>) -> Self {
        Self {
            condition: None,
            values: FlagValues::Complete(source.into()),
        }
    }
}
