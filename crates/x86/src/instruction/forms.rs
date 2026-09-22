mod binding;
pub(super) mod declarations;
mod encoding;
mod modrm;
mod opcodes;

#[cfg(test)]
mod tests;

pub(super) use declarations::instruction_families;
pub(crate) use encoding::{DecodedFields, Encoding, FieldWidth, ImmediateWidth, OperandEncoding};
pub(crate) use modrm::ModRmSelector;
pub(crate) use opcodes::forms_by_opcode;

use super::{
    handlers::{HandlerCall, SizedHandlers},
    Group1Prefix, OperandSize, PrefixState, SegmentOverride,
};
use crate::register::NamedRegister;
use crate::{address::AddressSize, flags::Condition};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum OpcodeMap {
    #[default]
    Primary,
    Extended,
}

impl OpcodeMap {
    pub(crate) const fn bytes(self) -> u32 {
        match self {
            Self::Primary => 1,
            Self::Extended => 2,
        }
    }
}

/// A location's role is independent of the fields needed to decode it.
#[derive(Clone, Copy)]
pub(super) enum LocationBinding {
    Register,
    Rm,
    /// A named register view, independent of any encoded register field.
    FixedRegister(NamedRegister),
    AbsoluteOffset,
}

#[derive(Clone, Copy)]
pub(super) enum OperandBinding {
    Location(LocationBinding),
    X87StackIndex,
    Segment(crate::Segment),
    Immediate(usize),
    /// An implicit literal, interpreted at the handler's logical operand width.
    Constant(u32),
    /// The r/m address fields form a value; register addressing is not accepted.
    RmAddress,
}

type HandlerBinding = HandlerCall<LocationBinding, OperandBinding>;

#[derive(Clone, Copy)]
pub(crate) struct Form {
    pub(super) opcode: u8,
    pub(super) mask: u8,
    pub(crate) map: OpcodeMap,
    /// Exact F2/F3 requirement; LOCK is admitted separately for eligible forms.
    group1_prefix: Option<Group1Prefix>,
    lockable: bool,
    pub(crate) encoding: Encoding,
    pub(crate) modrm: Option<ModRmSelector>,
    handlers: SizedHandlers<HandlerBinding>,
    pub(super) condition: Option<Condition>,
    pub(super) implicit_memory: bool,
    pub(super) ends_block: bool,
}

impl Form {
    /// Resolve prefix meaning before either decoder reads operand fields.
    pub(crate) fn resolve(&self, prefixes: &PrefixState) -> Option<ResolvedForm> {
        let accepts_prefix = match prefixes.group1() {
            Some(Group1Prefix::F0) => self.lockable,
            prefix => self.group1_prefix == prefix,
        };
        if !accepts_prefix {
            return None;
        }
        let operand_size = prefixes.operand_size();
        Some(ResolvedForm {
            form: *self,
            operand_size,
            address_size: prefixes.address_size(),
            call: self.handlers.resolve(operand_size),
            segment_override: prefixes.segment_override().clone(),
            locked: prefixes.group1() == Some(Group1Prefix::F0),
        })
    }

    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(crate) fn matches_modrm(&self, modrm: u8, prefixes: &PrefixState) -> bool {
        self.modrm.is_some_and(|selector| selector.matches(modrm))
            && (modrm >> 6 != 3 || self.accepts_register_rm(prefixes))
    }

    pub(crate) fn accepts_register_rm(&self, prefixes: &PrefixState) -> bool {
        self.modrm.is_some_and(ModRmSelector::accepts_register)
            && prefixes.group1() != Some(Group1Prefix::F0)
    }

    pub(crate) fn accepts_memory_rm(&self) -> bool {
        self.modrm.is_some_and(ModRmSelector::accepts_memory)
    }
}

/// Physical fetch widths and the concrete handler are selected from the prefix state.
#[derive(Clone)]
pub(crate) struct ResolvedForm {
    form: Form,
    operand_size: OperandSize,
    address_size: AddressSize,
    call: HandlerBinding,
    segment_override: SegmentOverride,
    locked: bool,
}

impl ResolvedForm {
    pub(crate) fn address_size(&self) -> AddressSize {
        self.address_size
    }

    pub(crate) fn address_width(&self) -> FieldWidth {
        match self.address_size {
            AddressSize::Bits16 => FieldWidth::Word,
            AddressSize::Bits32 => FieldWidth::Dword,
        }
    }

    pub(crate) fn encoding(&self) -> Encoding {
        self.form.encoding
    }

    pub(crate) fn immediate_width(&self, field: ImmediateWidth) -> FieldWidth {
        field.width(self.operand_size)
    }
}
