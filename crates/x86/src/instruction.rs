mod definitions;
mod forms;
mod handlers;
mod lower;
mod operands;
mod prefixes;

pub(crate) use definitions::opcode_forms;
pub(crate) use forms::*;
use handlers::HandlerCall;
pub(super) use lower::lower;
use operands::{map_location, map_operand};
pub(crate) use operands::{Input, TypedLocation, X87StackIndex};
pub(crate) use prefixes::{Group1Prefix, Prefix, PrefixState, SegmentOverride};

use crate::address::{AddressSize, EffectiveAddress, MemoryAddress};
use crate::flags::Condition;
use crate::register::RegisterOperand;
use crate::Segment;
use wasm86_compiler::{Val, I16, I32};

pub(super) const MAX_INSTRUCTION_BYTES: u32 = 15;
pub(super) const EXTENDED_OPCODE_ESCAPE: u8 = 0x0f;

/// The effective operand-size attribute after applying CS.D and prefixes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OperandSize {
    Word,
    Dword,
}

/// Decoded values and locations; handlers assign their logical widths.
pub(super) enum Operand<V> {
    Immediate(V),
    /// A logical x87 stack offset, independent of the current TOP.
    X87StackIndex(V),
    /// A segment register identity; its selector and cache have separate effects.
    Segment(Segment),
    /// The address value itself, without accessing the addressed memory.
    Address(EffectiveAddress<V>),
    Location(Location<V>),
}

#[derive(Clone)]
pub(super) enum Location<V> {
    Register(RegisterOperand),
    // Keep register operands compact while memory retains its full address terms.
    Memory(Box<MemoryAddress<V>>),
}

impl<V> From<Location<V>> for Operand<V> {
    fn from(location: Location<V>) -> Self {
        Self::Location(location)
    }
}

/// Handler arguments and the properties shared by every instruction shape.
pub(super) struct Instruction<V> {
    call: HandlerCall<Location<V>, Operand<V>>,
    condition: Option<Condition>,
    implicit_memory: bool,
    ends_block: bool,
    pub(super) address_size: AddressSize,
    pub(super) segment_override: SegmentOverride,
    pub(super) locked: bool,
    pub(super) x87_opcode: Option<X87Opcode<V>>,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) eip: P,
    /// The byte position after this instruction, before choosing a branch target.
    pub(super) fallthrough_eip: P,
}

/// Original x87 encoding bytes, retained for the architectural last opcode.
pub(crate) struct X87Opcode<V> {
    primary: u8,
    modrm: V,
}

impl<V: Into<Val<I32>>> X87Opcode<V> {
    pub(crate) fn bits(self) -> Val<I16> {
        self.modrm
            .into()
            .or(u32::from(self.primary & 7) << 8)
            .truncate()
    }
}

impl<V> Instruction<V> {
    pub(super) fn ends_block(&self) -> bool {
        self.ends_block
    }

    pub(super) fn uses_memory(&self) -> bool {
        self.implicit_memory
            || match &self.call {
                HandlerCall::Nullary { .. } => false,
                HandlerCall::Binary { left, right, .. } => {
                    left.uses_memory() || right.uses_memory()
                }
                HandlerCall::Unary { operand, .. } => operand.uses_memory(),
                HandlerCall::Ternary {
                    destination,
                    first_source,
                    second_source,
                    ..
                } => {
                    destination.uses_memory()
                        || first_source.uses_memory()
                        || second_source.uses_memory()
                }
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
