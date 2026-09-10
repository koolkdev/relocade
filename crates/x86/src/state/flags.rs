//! Admission and history of complete and partial symbolic flag changes.

mod publication;
mod queries;
pub(super) mod record;

use wasm86_compiler::{BuildError, FunctionBuilder, MemoryInt, Val, I1};

use crate::flags::{Condition, FlagChange, FlagMask, FlagSource, LocalFlagSource};

use super::State;
use queries::StoredFlagCache;

pub(super) fn condition_index(canonical: Condition) -> usize {
    Condition::CANONICAL
        .iter()
        .position(|candidate| *candidate == canonical)
        .expect("a canonical condition has a cache slot")
}

/// Changes follow one complete source in program order. Publication may use
/// descendant arms without changing this history or its query caches.
#[derive(Default)]
pub(super) struct FlagState {
    base: FlagBase,
    updates: Vec<FlagUpdate>,
}

#[derive(Clone)]
enum FlagBase {
    Stored(StoredFlagCache),
    Local(LocalFlagSource),
}

struct FlagUpdate {
    /// None denotes an unconditional change.
    condition: Option<Val<I1>>,
    change: FlagChange,
}

impl Default for FlagBase {
    fn default() -> Self {
        Self::Stored(StoredFlagCache::default())
    }
}

impl FlagState {
    fn apply(&mut self, condition: Option<Val<I1>>, change: FlagChange) {
        match (condition, change) {
            (None, FlagChange::Complete(source)) => {
                self.base = FlagBase::Local(source);
                self.updates.clear();
            }
            (condition, change) => self.updates.push(FlagUpdate { condition, change }),
        }
    }
}

impl State<'_> {
    /// Applies a symbolic flag change after the instruction's fault guards pass.
    pub(crate) fn set_flags(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        change: impl Into<FlagChange>,
    ) -> Result<(), BuildError> {
        let change = change.into();
        admit_change(body, &change)?;
        if change.writes() != FlagMask::EMPTY {
            self.flags.apply(None, change);
        }
        Ok(())
    }

    /// Applies a change only when the predicate is true; false retains the
    /// complete previous source, including an untouched stored record.
    pub(crate) fn set_flags_if(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: impl Into<Val<I1>>,
        change: impl Into<FlagChange>,
    ) -> Result<(), BuildError> {
        let condition = body.value(condition)?;
        let change = change.into();
        admit_change(body, &change)?;
        // Validate every provided value even when the predicate folds to false.
        if condition.same_expression(&body.value::<I1>(false)?)
            || change.writes() == FlagMask::EMPTY
        {
            return Ok(());
        }
        let condition = if condition.same_expression(&body.value::<I1>(true)?) {
            None
        } else {
            Some(condition)
        };
        self.flags.apply(condition, change);
        Ok(())
    }
}

fn admit_change(body: &FunctionBuilder<'_>, change: &FlagChange) -> Result<(), BuildError> {
    match change {
        FlagChange::Complete(source) => match source {
            LocalFlagSource::Byte(source) => admit_source(body, source),
            LocalFlagSource::Word(source) => admit_source(body, source),
            LocalFlagSource::Dword(source) => admit_source(body, source),
        },
        FlagChange::Partial(flags) => {
            for value in flags.iter().flatten() {
                body.value(value)?;
            }
            Ok(())
        }
    }
}

fn admit_source<T: MemoryInt>(
    body: &FunctionBuilder<'_>,
    source: &FlagSource<T>,
) -> Result<(), BuildError> {
    match source {
        FlagSource::Arithmetic {
            left,
            right,
            result,
            ..
        } => {
            body.value(left)?;
            body.value(right)?;
            body.value(result)?;
        }
        FlagSource::Explicit { result, flags } => {
            body.value(result)?;
            for flag in flags {
                body.value(flag)?;
            }
        }
        FlagSource::Logic { result } => {
            body.value(result)?;
        }
    }
    Ok(())
}
