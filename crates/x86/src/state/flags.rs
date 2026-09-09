//! Status sources, stored-record layout and publication of completed flags.

use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, MemoryInt, Val, I1, I32, I8};

use crate::flags::{
    ArithmeticKind, ArithmeticSource, Condition, FlagSource, LocalFlagSource, StatusFlag,
};

use super::State;

pub(super) const KIND_OFFSET: u32 = 0;
pub(super) const LEFT_OFFSET: u32 = 4;
pub(super) const RIGHT_OFFSET: u32 = 8;
pub(super) const CONCRETE_OFFSET: u32 = 12;

pub(super) const STATUS_FLAGS: [StatusFlag; 6] = [
    StatusFlag::CF,
    StatusFlag::PF,
    StatusFlag::AF,
    StatusFlag::ZF,
    StatusFlag::SF,
    StatusFlag::OF,
];

pub(super) fn status_index(flag: StatusFlag) -> usize {
    STATUS_FLAGS
        .iter()
        .position(|candidate| *candidate == flag)
        .expect("every status flag has a CPU byte")
}

pub(super) fn condition_index(canonical: Condition) -> usize {
    Condition::CANONICAL
        .iter()
        .position(|candidate| *candidate == canonical)
        .expect("a canonical condition has a cache slot")
}

pub(super) fn width_code<T: MemoryInt>() -> u8 {
    match T::BYTES {
        1 => 0,
        2 => 4,
        4 => 8,
        _ => unreachable!("x86 status sources have byte, word or dword operands"),
    }
}

pub(super) fn encode_kind<T: MemoryInt>(kind: ArithmeticKind) -> u8 {
    width_code::<T>()
        | match kind {
            ArithmeticKind::Sub => 1,
            ArithmeticKind::Add => 2,
        }
}

pub(super) fn encode_logic<T: MemoryInt>() -> u8 {
    width_code::<T>() | 3
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
    /// The concrete flag bytes remain untouched.
    pub(crate) fn set_arithmetic_flags<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        source: &ArithmeticSource<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        self.set_flag_source(body, FlagSource::Arithmetic(source.clone()))
    }

    /// Logical flags retain only the result. CF/OF are clear and undefined AF
    /// follows the zero policy; set them after every architectural guard.
    pub(crate) fn set_logic_flags<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        result: &Val<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        self.set_flag_source(
            body,
            FlagSource::Logic {
                result: result.clone(),
            },
        )
    }

    fn set_flag_source<T: MemoryInt>(
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
            FlagSource::Arithmetic(source) => {
                body.value(&source.left)?;
                body.value(&source.right)?;
                body.value(&source.result)?;
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
        match source {
            LocalFlagSource::Byte(source) => self.publish_flag_source(body, source),
            LocalFlagSource::Word(source) => self.publish_flag_source(body, source),
            LocalFlagSource::Dword(source) => self.publish_flag_source(body, source),
        }
    }

    fn publish_flag_source<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        source: &FlagSource<T>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let memory = self.cpu.memory();
        let kind = match source {
            FlagSource::Arithmetic(source) => {
                body.store(memory, LEFT_OFFSET, source.left.unsigned().extend::<I32>())?;
                body.store(
                    memory,
                    RIGHT_OFFSET,
                    source.right.unsigned().extend::<I32>(),
                )?;
                encode_kind::<T>(source.kind)
            }
            FlagSource::Logic { result } => {
                // A logical record leaves the unused right payload untouched.
                body.store(memory, LEFT_OFFSET, result.unsigned().extend::<I32>())?;
                encode_logic::<T>()
            }
        };
        // Write the tag after every payload it describes.
        body.store::<I8>(memory, KIND_OFFSET, u32::from(kind))
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
