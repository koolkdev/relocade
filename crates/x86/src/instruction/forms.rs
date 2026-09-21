mod binding;
pub(super) mod declarations;
mod encoding;
mod opcodes;

#[cfg(test)]
mod tests;

pub(super) use declarations::instruction_families;
pub(crate) use encoding::{DecodedFields, Encoding, FieldWidth, ImmediateWidth, OperandEncoding};
pub(crate) use opcodes::forms_by_opcode;

use super::{
    handlers::{HandlerCall, SizedHandlers},
    OperandSize, PrefixState, SegmentOverride,
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
    pub(crate) encoding: Encoding,
    /// Required ModRM.reg opcode extension; otherwise those bits belong to the encoding.
    pub(crate) extension: Option<u8>,
    handlers: SizedHandlers<HandlerBinding>,
    memory_only: bool,
    pub(super) condition: Option<Condition>,
    pub(super) implicit_memory: bool,
    pub(super) ends_block: bool,
    repeat_handlers: [Option<SizedHandlers<HandlerBinding>>; 2],
}

impl Form {
    /// Resolve prefix meaning before either decoder reads operand fields.
    pub(crate) fn resolve(&self, prefixes: &PrefixState) -> Option<ResolvedForm> {
        let operand_size = prefixes.operand_size();
        let handlers = if let Some(prefix) = prefixes.repeat() {
            self.repeat_handlers[prefix.index()]?
        } else {
            self.handlers
        };
        Some(ResolvedForm {
            form: *self,
            operand_size,
            address_size: prefixes.address_size(),
            call: handlers.resolve(operand_size),
            segment_override: prefixes.segment_override().clone(),
        })
    }

    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(crate) fn matches_modrm(&self, modrm: u8) -> bool {
        self.extension
            .is_none_or(|extension| ((modrm >> 3) & 7) == extension)
            && (modrm >> 6 != 3 || self.accepts_register_rm())
    }

    pub(crate) fn accepts_register_rm(&self) -> bool {
        !self.memory_only
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
