//! Current symbolic flag sources, admission and stored-condition caching.

pub(super) mod record;

use wasm86_compiler::{BuildError, FunctionBuilder, MemoryInt, Val, I1};

use crate::flags::{Condition, FlagSource, LocalFlagSource};

use super::State;
use record::FlagRecord;

pub(super) fn condition_index(canonical: Condition) -> usize {
    Condition::CANONICAL
        .iter()
        .position(|candidate| *candidate == canonical)
        .expect("a canonical condition has a cache slot")
}

/// Current status flags come from CPU backing or a locally computed source.
pub(super) enum FlagState {
    Stored {
        cached_conditions: [Option<Val<I1>>; Condition::CANONICAL.len()],
    },
    Local(LocalFlagSource),
}

impl Default for FlagState {
    fn default() -> Self {
        Self::Stored {
            cached_conditions: Condition::CANONICAL.map(|_| None),
        }
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
        // Check every retained value before replacing the current source. This
        // checks body ownership and scope without evaluating any flag expressions.
        match &source {
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
        self.flags = FlagState::Local(source.into());
        Ok(())
    }

    pub(super) fn publish_flags(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        let FlagState::Local(source) = &self.flags else {
            return Ok(());
        };
        let memory = self.cpu.memory();
        match source {
            LocalFlagSource::Byte(source) => FlagRecord::from_source(source).write(body, memory),
            LocalFlagSource::Word(source) => FlagRecord::from_source(source).write(body, memory),
            LocalFlagSource::Dword(source) => FlagRecord::from_source(source).write(body, memory),
        }
    }

    pub(crate) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        match &mut self.flags {
            FlagState::Local(source) => body.value(source.condition(condition)),
            FlagState::Stored { cached_conditions } => {
                let canonical = condition.canonical();
                let slot = &mut cached_conditions[condition_index(canonical)];
                let value = if let Some(value) = slot.as_ref() {
                    body.value(value)?
                } else {
                    let value = self.cpu.read_condition(body, canonical)?;
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
