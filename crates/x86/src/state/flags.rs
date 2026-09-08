//! Status flags from stored CPU records or locally computed arithmetic.
//! The SSA environment owns the stored definitions; this owner interprets them.

use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, MemoryInt, Val, I1, I32, I8};

use crate::{
    flags::{ArithmeticFlagSource, ArithmeticKind, ArithmeticSource, Condition, StatusFlag},
    ssa::Location,
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

/// Current status flags come from CPU backing or the latest local arithmetic.
pub(super) enum FlagState {
    Stored {
        cached_conditions: [Option<Val<I1>>; Condition::CANONICAL.len()],
    },
    Arithmetic(ArithmeticFlagSource),
}

impl Default for FlagState {
    fn default() -> Self {
        Self::Stored {
            cached_conditions: Condition::CANONICAL.map(|_| None),
        }
    }
}

impl State<'_> {
    /// Replaces all six status flags with this arithmetic source, retaining it
    /// for condition queries. Call only after the instruction's fault guards pass;
    /// the concrete flag bytes remain untouched.
    pub(crate) fn set_arithmetic_flags<T: MemoryInt>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        source: &ArithmeticSource<T>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
        ArithmeticSource<T>: Into<ArithmeticFlagSource>,
    {
        self.values.define(
            body,
            Location::<I32>::new(LEFT_OFFSET),
            source.left.unsigned().extend::<I32>(),
        )?;
        self.values.define(
            body,
            Location::<I32>::new(RIGHT_OFFSET),
            source.right.unsigned().extend::<I32>(),
        )?;
        self.values.define(
            body,
            Location::<I8>::new(KIND_OFFSET),
            u32::from(encode_kind::<T>(source.kind)),
        )?;
        self.flags = FlagState::Arithmetic(source.clone().into());
        Ok(())
    }

    pub(crate) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        match &mut self.flags {
            FlagState::Arithmetic(source) => body.value(source.condition(condition)),
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
