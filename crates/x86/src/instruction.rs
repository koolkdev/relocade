mod definitions;
mod forms;
mod handlers;
mod lower;
mod operands;
mod prefixes;
mod requirements;

pub(crate) use definitions::opcode_forms;
pub use forms::CallTarget;
pub(crate) use forms::*;
use handlers::HandlerCall;
pub(super) use lower::lower;
use operands::{map_location, map_operand};
pub(crate) use operands::{
    Input, MmxLocation, PackedShiftCount, TypedLocation, VectorAlignment, X87StackIndex,
    XmmLocation, XmmType,
};
pub(crate) use prefixes::{FormSelection, Group1Prefix, Prefix, PrefixState, SegmentOverride};
use requirements::{Availability, Requirements};

use crate::address::{EffectiveAddress, MemoryAddress};
use crate::flags::Condition;
use crate::register::RegisterOperand;
use crate::Segment;
use relocade_compiler::{Val, I16, I32};

/// Maximum length of one x86 instruction, including all prefixes and operands.
pub const MAX_INSTRUCTION_BYTES: u32 = 15;
pub(super) const EXTENDED_OPCODE_ESCAPE: u8 = 0x0f;

/// Control flow of a decoded instruction under the supplied execution target.
/// Offsets are relative to the current CS. Execution can fault before a transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionFlow {
    /// Execution can continue at the next encoded instruction in the same block.
    Continue,
    /// This instruction ends its block and can resume at the next encoded instruction.
    Fallthrough,
    /// A direct near jump.
    Jump { target: u32 },
    /// A conditional near transfer. The untaken path uses the next encoded EIP.
    Branch { taken: u32 },
    /// A near call saves the operand-sized return EIP in the caller's CS.
    NearCall { return_eip: u32 },
    /// For an ordinary far call, the operand-sized return EIP in the caller's CS.
    /// Protected-mode far transfers require resolving the destination descriptor.
    FarCall { return_eip: u32 },
    /// This instruction ends its block with no statically known continuation in this CS.
    Stop,
}

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
    Mmx(crate::register::RegisterSelector),
    Xmm(crate::register::RegisterSelector),
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
    ends_block: bool,
    requirements: Requirements,
    pub(super) address_mask: V,
    pub(super) segment_override: SegmentOverride,
    pub(super) locked: bool,
    pub(super) opcode: u8,
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
}
