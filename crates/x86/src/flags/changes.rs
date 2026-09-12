//! Explicit write sets preserve status recipes and unrelated architectural flags.

use wasm86_compiler::{MemoryInt, Val, I1};

use super::{Flag, FlagMask};
use crate::alu::{AnyStatusSource, StatusSource};

pub(crate) struct FlagChange {
    /// None denotes an unconditional change.
    pub(crate) condition: Option<Val<I1>>,
    pub(crate) values: FlagValues,
    writes: FlagMask,
}

pub(crate) enum FlagValues {
    Status(AnyStatusSource),
    Explicit([Option<Val<I1>>; Flag::ALL.len()]),
}

impl FlagChange {
    /// Omitted flags retain their previous values.
    pub(crate) fn partial<const N: usize>(flags: [(Flag, Val<I1>); N]) -> Self {
        let mut values = Flag::ALL.map(|_| None);
        let mut writes = FlagMask::EMPTY;
        for (flag, value) in flags {
            values[flag.index()] = Some(value);
            writes = writes.union(FlagMask::of(flag));
        }
        Self {
            condition: None,
            values: FlagValues::Explicit(values),
            writes,
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

    pub(crate) fn writes(&self) -> FlagMask {
        self.writes
    }

    pub(crate) fn flag(&self, flag: impl Into<Flag>) -> Val<I1> {
        let flag = flag.into();
        assert!(
            self.writes.contains(flag),
            "the change defines the requested flag"
        );
        match &self.values {
            FlagValues::Status(source) => {
                let Flag::Status(flag) = flag else {
                    unreachable!("status sources only write status flags")
                };
                source.flag(flag)
            }
            FlagValues::Explicit(flags) => flags[flag.index()].as_ref().unwrap().clone(),
        }
    }

    /// Leaves this flag unchanged without expanding a retained status recipe.
    pub(crate) fn preserving(self, flag: impl Into<Flag>) -> Self {
        self.retaining(FlagMask::ALL.without(flag))
    }

    /// Restricts the write set without changing the retained source or predicate.
    pub(crate) fn retaining(mut self, mask: FlagMask) -> Self {
        self.writes = self.writes.intersection(mask);
        if let FlagValues::Explicit(values) = &mut self.values {
            for flag in FlagMask::ALL.flags() {
                if !self.writes.contains(flag) {
                    values[flag.index()] = None;
                }
            }
        }
        self
    }

    /// A recipe can answer a condition directly only if it supplies every dependency.
    pub(crate) fn status_source(&self, needed: FlagMask) -> Option<&AnyStatusSource> {
        match &self.values {
            FlagValues::Status(source) if self.writes.covers(needed) => Some(source),
            _ => None,
        }
    }
}

impl<T: MemoryInt> From<StatusSource<T>> for FlagChange
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    fn from(source: StatusSource<T>) -> Self {
        Self {
            condition: None,
            values: FlagValues::Status(source.into()),
            writes: FlagMask::STATUS,
        }
    }
}
