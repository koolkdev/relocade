//! Current symbolic flag sources, admission and stored-condition caching.

pub(super) mod record;

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryInt, Val, I1};

use crate::flags::{Condition, FlagSource, LocalFlagSource};

use super::{Cpu, State};
use record::FlagRecord;

pub(super) fn condition_index(canonical: Condition) -> usize {
    Condition::CANONICAL
        .iter()
        .position(|candidate| *candidate == canonical)
        .expect("a canonical condition has a cache slot")
}

/// One unconditional source followed by conditional replacements in program order.
/// Definitions and queries stay on the execution path; terminal publication may
/// use descendant arms without changing this history or its cached conditions.
#[derive(Default)]
pub(super) struct FlagState {
    base: FlagBase,
    updates: Vec<ConditionalFlagUpdate>,
}

enum FlagBase {
    Stored {
        cached_conditions: [Option<Val<I1>>; Condition::CANONICAL.len()],
    },
    Local(LocalFlagSource),
}

struct ConditionalFlagUpdate {
    condition: Val<I1>,
    source: LocalFlagSource,
}

impl Default for FlagBase {
    fn default() -> Self {
        Self::Stored {
            cached_conditions: Condition::CANONICAL.map(|_| None),
        }
    }
}

impl FlagState {
    fn replace(&mut self, source: LocalFlagSource) {
        self.base = FlagBase::Local(source);
        self.updates.clear();
    }
}

impl State<'_> {
    /// Replaces all six status flags after the instruction's fault guards pass.
    /// Retains a symbolic source without choosing or writing a CPU record yet.
    pub(crate) fn set_flags<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        source: FlagSource<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        admit_source(body, &source)?;
        self.flags.replace(source.into());
        Ok(())
    }

    /// Replaces all six flags only when the predicate is true. A false predicate
    /// keeps the complete previous source, including an untouched stored record.
    /// As with unconditional replacement, all guest fault guards must pass first.
    pub(crate) fn set_flags_if<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: impl Into<Val<I1>>,
        source: FlagSource<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        let condition = body.value(condition)?;
        admit_source(body, &source)?;
        // Admitted constants share their arena representation, including folded
        // expressions. Validate the source even when its predicate is false.
        if condition.same_expression(&body.value::<I1>(false)?) {
            return Ok(());
        }
        let source = source.into();
        if condition.same_expression(&body.value::<I1>(true)?) {
            self.flags.replace(source);
        } else {
            self.flags
                .updates
                .push(ConditionalFlagUpdate { condition, source });
        }
        Ok(())
    }

    pub(super) fn publish_flags(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        let memory = self.cpu.memory();
        if self.flags.updates.is_empty() {
            return self.flags.base.publish(body, memory);
        }
        // The last active replacement owns the record. Outward branches avoid
        // nesting one control region per update and skip every older payload.
        body.block::<()>(|mut block, done| {
            for update in self.flags.updates.iter().rev() {
                block.if_(&update.condition, |mut arm| {
                    publish_source(&mut arm, memory, &update.source)?;
                    arm.branch(&done, ())
                })?;
            }
            self.flags.base.publish(&mut block, memory)
        })
    }

    pub(crate) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let mut value = self.flags.base.condition(body, self.cpu, condition)?;
        for update in &self.flags.updates {
            value = update
                .condition
                .select(update.source.condition(condition), value);
        }
        body.value(value)
    }
}

impl FlagBase {
    fn publish(&self, body: &mut FunctionBuilder<'_>, memory: Mem) -> Result<(), BuildError> {
        match self {
            Self::Stored { .. } => Ok(()),
            Self::Local(source) => publish_source(body, memory, source),
        }
    }

    fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        match self {
            Self::Local(source) => body.value(source.condition(condition)),
            Self::Stored { cached_conditions } => {
                let canonical = condition.canonical();
                let slot = &mut cached_conditions[condition_index(canonical)];
                let value = if let Some(value) = slot.as_ref() {
                    body.value(value)?
                } else {
                    let value = cpu.read_condition(body, canonical)?;
                    *slot = Some(value.clone());
                    value
                };
                Ok(if condition.is_inverted() {
                    value.eq(0)
                } else {
                    value
                })
            }
        }
    }
}

fn admit_source<T: MemoryInt>(
    body: &FunctionBuilder<'_>,
    source: &FlagSource<T>,
) -> Result<(), BuildError> {
    // Admission checks body ownership and scope without evaluating flag rules.
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

fn publish_source(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
    source: &LocalFlagSource,
) -> Result<(), BuildError> {
    match source {
        LocalFlagSource::Byte(source) => FlagRecord::from_source(source).write(body, memory),
        LocalFlagSource::Word(source) => FlagRecord::from_source(source).write(body, memory),
        LocalFlagSource::Dword(source) => FlagRecord::from_source(source).write(body, memory),
    }
}
