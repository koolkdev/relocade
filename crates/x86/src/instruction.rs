mod definitions;
mod forms;
mod handlers;
mod lower;
mod operands;

pub(crate) use definitions::{modrm_forms, opcode_forms};
pub(crate) use forms::*;
use handlers::HandlerCall;
pub(super) use lower::lower;
use operands::{map_location, map_operand};
pub(crate) use operands::{Input, TypedLocation};

use crate::{address::Address32, flags::Condition, register::RegisterCode};

pub(super) const MAX_INSTRUCTION_BYTES: u32 = 15;
pub(super) const OPERAND_SIZE_PREFIX: u8 = 0x66;
pub(super) const EXTENDED_OPCODE_ESCAPE: u8 = 0x0f;

/// The effective operand-size attribute in the supported default-32 mode.
#[derive(Clone, Copy)]
pub(super) enum OperandSize {
    Word,
    Dword,
}

/// Decoded bits and locations; handlers assign their logical widths.
pub(super) enum Operand<V> {
    Immediate(V),
    Location(Location<V>),
}

#[derive(Clone)]
pub(super) enum Location<V> {
    Register(RegisterCode),
    Memory(Address32<V>),
}

impl<V> From<Location<V>> for Operand<V> {
    fn from(location: Location<V>) -> Self {
        Self::Location(location)
    }
}

/// Handler arguments and the properties shared by every instruction shape.
pub(super) struct Instruction<V> {
    call: HandlerCall<V>,
    condition: Option<Condition>,
    implicit_memory: bool,
    ends_block: bool,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) eip: P,
    /// The byte position after this instruction, before choosing a branch target.
    pub(super) fallthrough_eip: P,
}

impl<V> Instruction<V> {
    pub(super) fn ends_block(&self) -> bool {
        self.ends_block
    }

    pub(super) fn uses_memory(&self) -> bool {
        self.implicit_memory
            || match &self.call {
                HandlerCall::Binary { left, right, .. } => {
                    left.uses_memory() || right.uses_memory()
                }
                HandlerCall::Unary { operand, .. } => operand.uses_memory(),
            }
    }
}

impl<V> Location<V> {
    fn uses_memory(&self) -> bool {
        matches!(self, Self::Memory(_))
    }
}

impl<V> Operand<V> {
    fn uses_memory(&self) -> bool {
        matches!(self, Self::Location(location) if location.uses_memory())
    }
}
