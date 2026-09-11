//! Admission and history of complete and partial symbolic flag changes.

mod publication;
mod queries;
pub(super) mod record;

use wasm86_compiler::{BuildError, FunctionBuilder, MemoryInt, I1};

use crate::alu::flags::{AnyFlagSource, Condition, FlagChange, FlagMask, FlagSource, FlagValues};

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
/// History contains only nonempty changes; any predicate is admitted and nonconstant.
/// An unconditional complete change replaces the base instead of entering history.
#[derive(Default)]
pub(super) struct FlagState {
    base: FlagBase,
    updates: Vec<FlagChange>,
}

#[derive(Clone)]
enum FlagBase {
    Stored(StoredFlagCache),
    Local(AnyFlagSource),
}

impl Default for FlagBase {
    fn default() -> Self {
        Self::Stored(StoredFlagCache::default())
    }
}

impl FlagState {
    fn apply(&mut self, change: FlagChange) {
        match change {
            FlagChange {
                condition: None,
                values: FlagValues::Complete(source),
            } => {
                self.base = FlagBase::Local(source);
                self.updates.clear();
            }
            change => self.updates.push(change),
        }
    }
}

impl State<'_> {
    /// Applies a symbolic flag change after the instruction's fault guards pass.
    /// A false condition retains the entire previous source and stored record.
    pub(crate) fn set_flags(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        change: impl Into<FlagChange>,
    ) -> Result<(), BuildError> {
        let mut change = change.into();
        change.condition = change
            .condition
            .map(|condition| body.value(condition))
            .transpose()?;
        admit_values(body, &change.values)?;
        // Admit every provided value before omitting an empty or false change.
        if change.writes() == FlagMask::EMPTY {
            return Ok(());
        }
        if let Some(condition) = &change.condition {
            if condition.same_expression(&body.value::<I1>(false)?) {
                return Ok(());
            }
            if condition.same_expression(&body.value::<I1>(true)?) {
                change.condition = None;
            }
        }
        self.flags.apply(change);
        Ok(())
    }
}

fn admit_values(body: &FunctionBuilder<'_>, values: &FlagValues) -> Result<(), BuildError> {
    match values {
        FlagValues::Complete(source) => match source {
            AnyFlagSource::Byte(source) => admit_source(body, source),
            AnyFlagSource::Word(source) => admit_source(body, source),
            AnyFlagSource::Dword(source) => admit_source(body, source),
        },
        FlagValues::Partial(flags) => {
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
        FlagSource::Explicit { flags } => {
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
