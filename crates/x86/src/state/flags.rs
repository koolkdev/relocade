//! Architectural flag state owns admission, queries and terminal publication.

mod publication;
mod queries;
pub(super) mod record;

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryInt, Val, I1, I8};

use crate::{
    alu::{AnyStatusSource, StatusSource},
    flags::{Condition, Flag, FlagChange, FlagMask, FlagValues},
    ssa::Environment,
};

use super::{access::cpu_location, Cpu};
use queries::StoredFlagCache;

pub(super) fn condition_index(canonical: Condition) -> usize {
    Condition::CANONICAL
        .iter()
        .position(|candidate| *candidate == canonical)
        .expect("a canonical condition has a cache slot")
}

pub(super) struct FlagState {
    status: StatusState,
    direct: Environment,
}

/// Status changes follow one complete status source in program order.
/// Unconditional replacements discard only the older status history.
#[derive(Default)]
struct StatusState {
    base: StatusBase,
    updates: Vec<FlagChange>,
}

#[derive(Clone)]
enum StatusBase {
    Stored(StoredFlagCache),
    Local(AnyStatusSource),
}

impl Default for StatusBase {
    fn default() -> Self {
        Self::Stored(StoredFlagCache::default())
    }
}

impl FlagState {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            status: StatusState::default(),
            direct: Environment::new(memory),
        }
    }

    /// Admit the whole change before modifying either status or direct flag state.
    /// A false change preserves every backing byte, including noncanonical flags.
    pub(super) fn apply(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        mut change: FlagChange,
    ) -> Result<(), BuildError> {
        change.condition = change
            .condition
            .map(|condition| body.value(condition))
            .transpose()?;
        admit_values(body, &change.values)?;
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
        if change.writes().contains(Flag::DF) {
            let value = change.flag(Flag::DF).unsigned().extend::<I8>();
            let value = match &change.condition {
                Some(condition) => {
                    let old = self.direct.read(body, cpu_location!(flags.bytes.df))?;
                    condition.select(value, old)
                }
                None => value,
            };
            self.direct
                .define(body, cpu_location!(flags.bytes.df), value)?;
            change = change.retaining(FlagMask::STATUS);
        }
        if change.writes() != FlagMask::EMPTY {
            self.status.apply(change);
        }
        Ok(())
    }

    pub(super) fn read(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        flag: Flag,
    ) -> Result<Val<I1>, BuildError> {
        match flag {
            Flag::Status(flag) => self.status.read_flag(body, cpu, flag),
            Flag::DF => self
                .direct
                .read(body, cpu_location!(flags.bytes.df))
                .map(|value| value.truncate::<I1>()),
        }
    }

    pub(super) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        self.status.condition(body, cpu, condition)
    }
}

impl StatusState {
    fn apply(&mut self, change: FlagChange) {
        if change.condition.is_none() && change.writes() == FlagMask::STATUS {
            if let FlagValues::Status(source) = change.values {
                self.base = StatusBase::Local(source);
                self.updates.clear();
                return;
            }
        }
        self.updates.push(change);
    }
}

fn admit_values(body: &FunctionBuilder<'_>, values: &FlagValues) -> Result<(), BuildError> {
    match values {
        FlagValues::Status(source) => match source {
            AnyStatusSource::Byte(source) => admit_source(body, source),
            AnyStatusSource::Word(source) => admit_source(body, source),
            AnyStatusSource::Dword(source) => admit_source(body, source),
        },
        FlagValues::Explicit(flags) => {
            for value in flags.iter().flatten() {
                body.value(value)?;
            }
            Ok(())
        }
    }
}

fn admit_source<T: MemoryInt>(
    body: &FunctionBuilder<'_>,
    source: &StatusSource<T>,
) -> Result<(), BuildError> {
    match source {
        StatusSource::Arithmetic {
            left,
            right,
            result,
            ..
        } => {
            body.value(left)?;
            body.value(right)?;
            body.value(result)?;
        }
        StatusSource::Explicit { flags } => {
            for flag in flags {
                body.value(flag)?;
            }
        }
        StatusSource::Logic { result } => {
            body.value(result)?;
        }
    }
    Ok(())
}
